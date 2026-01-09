# spoutdx-ffi の使い方（DLL 利用ガイド）

このドキュメントは、ビルド済みの `spoutdx_ffi.dll` を **他プロジェクト（Rust / C / C++ 等）から利用する際の手順・注意点**をまとめたものです。

- プロジェクト概要・ビルド手順は README を参照してください。
- ここでは「利用側がどんな落とし穴に遭遇しやすいか」を中心に、設計に沿って説明します。

## 前提と設計の要点

- 対象は Windows + DirectX 11（D3D11 / DXGI）です。
- 公開 API は **純 C ABI** で、C++ の ABI 問題（STL 型など）を境界から排除しています。
- 受信（Receiver）機能は、Spout2 の DirectX 11 実装（SpoutDX）を内部に統合して動作します。

## 依存物と成果物

- DLL: `spoutdx_ffi.dll`
- ヘッダ: `include/spoutdx_ffi/spoutdx_ffi.h`

利用側は以下を行います。

- DLL を実行時に見つけられる場所へ配置（同ディレクトリ、PATH、適切なロードパスなど）
- ヘッダに書かれた関数を FFI で呼び出す

## エラーハンドリング（返り値の解釈）

Receiver API の多くは `int` を返し、成功は `SPOUTDX_OK (0)`、失敗は負値です。

- `SPOUTDX_ERROR_NULL_HANDLE`: ハンドルが不正
- `SPOUTDX_ERROR_NULL_DEVICE`: `ID3D11Device*` が NULL
- `SPOUTDX_ERROR_NOT_CONNECTED`: センダー未接続（受信前に起こりやすい）
- `SPOUTDX_ERROR_INIT_FAILED`: DX11 初期化失敗
- `SPOUTDX_ERROR_RECEIVE_FAILED`: 受信失敗
- `SPOUTDX_ERROR_INTERNAL`: 内部例外や想定外

運用上は「未接続」「更新中（サイズ変更直後）」「新規フレームなし」を区別して扱えるよう、状態取得 API を併用するのが安全です。

## Receiver の基本フロー（推奨）

以下は「設計に沿った、安全に動く」受信の流れです。

1. `spoutdx_receiver_create` でハンドル作成
2. 利用側で `ID3D11Device*` を作成（同一アダプタが前提）
3. `spoutdx_receiver_open_dx11(handle, device)`
4. 必要なら `spoutdx_receiver_set_sender_name(handle, ...)` で対象センダーを固定
5. ループで受信
   - `spoutdx_receiver_receive(handle)` を呼ぶ（内部テクスチャに受信）
   - `spoutdx_receiver_is_connected(handle)` で接続状況を確認
   - `spoutdx_receiver_is_updated(handle)` を監視（サイズ/フォーマット変更の可能性）
   - `spoutdx_receiver_is_frame_new(handle)` を監視（新規フレームか）
   - `spoutdx_receiver_get_sender_info(handle, &info)` で `width/height/format` を取得
   - `spoutdx_receiver_get_received_texture(handle)` で内部テクスチャ取得
   - （必要なら）GPU→CPU 読み戻し、または自前の処理へ渡す
6. 終了時: `spoutdx_receiver_close_dx11` → `spoutdx_receiver_destroy`

ポイントは、利用側が「受信先テクスチャを必ず自分で用意する」よりも、まずは **内部受信（`spoutdx_receiver_receive`）＋内部テクスチャ取得** を基本にすることです。

## 重要: `IsUpdated`（更新フラグ）と受信の落とし穴

SpoutDX の受信フローでは「センダーのサイズ/フォーマットが変わった直後」などに **更新フラグ**が立ちます。

- 更新フラグが立っている間は、受信側でリソースの作り直しが必要になることがあります。
- そのタイミングで「利用側が用意した古いサイズのテクスチャへコピーしよう」とすると、失敗・黒画面・意図しない挙動になり得ます。

このプロジェクトでは、安定性重視のために以下の API を提供しています。

- `spoutdx_receiver_receive(handle)`
  - 受信を **SpoutDX 内部テクスチャ**で完結させる
- `spoutdx_receiver_get_received_texture(handle)`
  - 内部テクスチャ（`ID3D11Texture2D*`）を取得
- `spoutdx_receiver_get_dx11_context(handle)`
  - コピーなどに使われる **SpoutDX 側の D3D11 context** を取得

利用側は「更新中は寸法・フォーマットが変わり得る」前提で、`get_sender_info` を見て自前の staging/出力先を作り直してください。

## 重要: D3D11 デバイス/コンテキストの整合性

DirectX の `ID3D11Texture2D` は **生成したデバイスに紐づく**リソースです。

- 受信したテクスチャを別デバイスのコンテキストで `CopyResource` するのは不可です。
- 同一デバイスでも「どのコンテキストでコピーするか」が絡むケースがあり、デバッグで混乱しがちです。

このプロジェクトでは `spoutdx_receiver_get_dx11_context` を提供しているため、
受信テクスチャから staging への `CopyResource` などは **その context を使う**のが最も確実です。

## 重要: DXGI フォーマット（BGRA / RGBA）と色の入れ替わり

Spout センダーのフォーマットは `SpoutDxSenderInfo.format`（DXGI_FORMAT）として取得できます。

典型的に遭遇するのは次の 2 つです。

- `DXGI_FORMAT_B8G8R8A8_UNORM`（一般に BGRA として扱う）
- `DXGI_FORMAT_R8G8B8A8_UNORM`（一般に RGBA として扱う）

利用側で GPU→CPU 読み戻しや画像保存を行う場合、
**「受信テクスチャの実フォーマット」に合わせて staging も同じフォーマットで作り、CPU 側の解釈も合わせる**のが安全です。

## リソース寿命（ポインタの扱い）

- `SpoutDxReceiverHandle` は不透明ハンドルです。destroy 後に使用しない。
- `spoutdx_receiver_get_received_texture` で得たテクスチャポインタは、
  **receiver が生存している間のみ有効**です。
- `IsUpdated` が立った後などは内部テクスチャが作り直される可能性があるため、
  取得したポインタを長期間保持せず、必要に応じて取り直してください。

## よくあるトラブルシュート

### 接続できない / すぐ切れる

- `spoutdx_receiver_set_sender_name(NULL)`（アクティブセンダー）と固定名指定を切り替えて確認
- 送信側（Spout sender）が DX11 で動いているか、同一GPUかを確認

### 受信できるが黒い

- 更新フラグ（`spoutdx_receiver_is_updated`）のタイミングで寸法・フォーマットが変わっていないか
- 受信先を「外部テクスチャへコピー」方式にしている場合は、
  内部受信（`spoutdx_receiver_receive`）へ切り替えて挙動を分離

