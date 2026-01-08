````markdown
# Infrastructure層: Spout受信実装（SpoutCaptureAdapter）

このドキュメントは、**Spout DX11テクスチャ受信**による代替キャプチャ実装の設計と実装計画をまとめます。

## 概要

### 目的

DDAキャプチャの代替として、Spout送信されたDX11テクスチャを受信する機能を追加します。
これにより、以下のユースケースに対応できます：

- ゲーム側がSpout送信をサポートしている場合、DDAよりも効率的なテクスチャ取得
- 複数アプリケーション間でのテクスチャ共有シナリオ
- DDA利用時の制約（管理者権限、排他的フルスクリーン等）を回避

### Spoutとは

Spout は Windows 環境で DirectX テクスチャをプロセス間で共有するためのフレームワークです。
- DirectX 11 の共有テクスチャ機能を利用
- GPU上でゼロコピーに近い転送が可能（GPU→GPUコピー）
- 送信側・受信側は同じグラフィックスアダプタを使用する必要がある

## 対象範囲

- 実装: `SpoutCaptureAdapter`（`CapturePort` 実装）
- FFIバインディング: `third_party/spoutdx-ffi` を使用
- 設定: `config.toml` の `capture.source` で DDA/Spout を選択

## アーキテクチャ上の位置づけ

```
┌───────────────────────────────┐
│  Domain (CapturePort trait)    │  ← 変更なし
└──────────────▲────────────────┘
               │ trait実装
┌──────────────┴────────────────┐
│  Infrastructure/Adapters       │
│  ├─ DdaCaptureAdapter         │  ← 既存
│  └─ SpoutCaptureAdapter       │  ← 新規追加
└───────────────────────────────┘
```

Clean Architectureの原則に従い：
- **Domain層**: `CapturePort` trait は変更不要
- **Application層**: アダプタの選択ロジックのみ追加（DIで注入）
- **Infrastructure層**: `SpoutCaptureAdapter` を新規実装
- **Presentation層**: 設定に基づくアダプタ選択をmain.rsに追加

## FFI API（spoutdx-ffi）

`third_party/spoutdx-ffi/include/spoutdx_ffi.h` で定義されたC ABI：

### ライフサイクル

```c
SpoutDxReceiverHandle spoutdx_receiver_create(void);
int spoutdx_receiver_destroy(SpoutDxReceiverHandle handle);
```

### DirectX初期化

```c
int spoutdx_receiver_open_dx11(SpoutDxReceiverHandle handle, void* device);
int spoutdx_receiver_close_dx11(SpoutDxReceiverHandle handle);
```

- `device`: こちら側で作成した `ID3D11Device*` を渡す

### 受信設定

```c
int spoutdx_receiver_set_sender_name(SpoutDxReceiverHandle handle, const char* sender_name);
```

- `sender_name`: 接続する送信者名（NULLで最初のアクティブ送信者に自動接続）

### テクスチャ受信

```c
int spoutdx_receiver_receive_texture(SpoutDxReceiverHandle handle, void* dst_texture);
int spoutdx_receiver_release(SpoutDxReceiverHandle handle);
```

- `dst_texture`: こちら側で作成した `ID3D11Texture2D*` を渡す
- **重要**: テクスチャフォーマットとサイズは送信側と一致させる必要がある

### 状態クエリ

```c
int spoutdx_receiver_get_sender_info(SpoutDxReceiverHandle handle, SpoutDxSenderInfo* out_info);
int spoutdx_receiver_is_updated(SpoutDxReceiverHandle handle);
int spoutdx_receiver_is_connected(SpoutDxReceiverHandle handle);
int spoutdx_receiver_is_frame_new(SpoutDxReceiverHandle handle);
```

### SpoutDxSenderInfo構造体

```c
typedef struct SpoutDxSenderInfo {
    char name[256];
    unsigned int width;
    unsigned int height;
    unsigned int format;  // DXGI_FORMAT
} SpoutDxSenderInfo;
```

### 戻り値（SpoutDxResult）

```c
typedef enum SpoutDxResult {
    SPOUTDX_OK                   = 0,
    SPOUTDX_ERROR_NULL_HANDLE    = -1,
    SPOUTDX_ERROR_NULL_DEVICE    = -2,
    SPOUTDX_ERROR_NOT_CONNECTED  = -3,
    SPOUTDX_ERROR_INIT_FAILED    = -4,
    SPOUTDX_ERROR_RECEIVE_FAILED = -5,
    SPOUTDX_ERROR_INTERNAL       = -99
} SpoutDxResult;
```

## 実装計画

### Phase 1: FFI Rustバインディング

#### 1.1 ファイル構成

```
src/infrastructure/capture/
├── mod.rs            # CaptureSource enum追加、条件付きエクスポート
├── dda.rs            # 既存（変更なし）
├── spout.rs          # 新規: SpoutCaptureAdapter
└── spout_ffi.rs      # 新規: FFIバインディング
```

#### 1.2 spout_ffi.rs の実装

```rust
//! Spout DX11 FFI バインディング
//!
//! third_party/spoutdx-ffi のC APIをRustから呼び出すための
//! 安全なラッパーを提供します。

use std::ffi::{c_char, c_int, c_uint, c_void};
use std::ptr;

/// FFI戻り値
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpoutDxResult {
    Ok = 0,
    ErrorNullHandle = -1,
    ErrorNullDevice = -2,
    ErrorNotConnected = -3,
    ErrorInitFailed = -4,
    ErrorReceiveFailed = -5,
    ErrorInternal = -99,
}

impl SpoutDxResult {
    pub fn is_ok(self) -> bool {
        self == SpoutDxResult::Ok
    }
    
    pub fn from_raw(value: c_int) -> Self {
        match value {
            0 => SpoutDxResult::Ok,
            -1 => SpoutDxResult::ErrorNullHandle,
            -2 => SpoutDxResult::ErrorNullDevice,
            -3 => SpoutDxResult::ErrorNotConnected,
            -4 => SpoutDxResult::ErrorInitFailed,
            -5 => SpoutDxResult::ErrorReceiveFailed,
            _ => SpoutDxResult::ErrorInternal,
        }
    }
}

/// 送信者情報
#[repr(C)]
pub struct SpoutDxSenderInfo {
    pub name: [c_char; 256],
    pub width: c_uint,
    pub height: c_uint,
    pub format: c_uint,
}

impl Default for SpoutDxSenderInfo {
    fn default() -> Self {
        Self {
            name: [0; 256],
            width: 0,
            height: 0,
            format: 0,
        }
    }
}

impl SpoutDxSenderInfo {
    pub fn name_as_string(&self) -> String {
        let bytes: Vec<u8> = self.name.iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as u8)
            .collect();
        String::from_utf8_lossy(&bytes).to_string()
    }
}

pub type SpoutDxReceiverHandle = *mut c_void;

#[link(name = "spoutdx_ffi")]
extern "C" {
    pub fn spoutdx_ffi_version() -> *const c_char;
    pub fn spoutdx_ffi_get_sdk_version() -> c_int;
    pub fn spoutdx_ffi_test_dx11_init() -> c_int;

    pub fn spoutdx_receiver_create() -> SpoutDxReceiverHandle;
    pub fn spoutdx_receiver_destroy(handle: SpoutDxReceiverHandle) -> c_int;

    pub fn spoutdx_receiver_open_dx11(
        handle: SpoutDxReceiverHandle,
        device: *mut c_void,
    ) -> c_int;
    pub fn spoutdx_receiver_close_dx11(handle: SpoutDxReceiverHandle) -> c_int;

    pub fn spoutdx_receiver_set_sender_name(
        handle: SpoutDxReceiverHandle,
        sender_name: *const c_char,
    ) -> c_int;

    pub fn spoutdx_receiver_receive_texture(
        handle: SpoutDxReceiverHandle,
        dst_texture: *mut c_void,
    ) -> c_int;
    
    pub fn spoutdx_receiver_release(handle: SpoutDxReceiverHandle) -> c_int;

    pub fn spoutdx_receiver_get_sender_info(
        handle: SpoutDxReceiverHandle,
        out_info: *mut SpoutDxSenderInfo,
    ) -> c_int;
    
    pub fn spoutdx_receiver_is_updated(handle: SpoutDxReceiverHandle) -> c_int;
    pub fn spoutdx_receiver_is_connected(handle: SpoutDxReceiverHandle) -> c_int;
    pub fn spoutdx_receiver_is_frame_new(handle: SpoutDxReceiverHandle) -> c_int;
}
```

### Phase 2: SpoutCaptureAdapter実装

#### 2.1 spout.rs の実装

```rust
//! Spout DX11 テクスチャ受信によるキャプチャアダプタ
//!
//! Spout送信されたDirectX 11テクスチャを受信し、
//! CapturePort traitを実装します。

use crate::domain::{CapturePort, DeviceInfo, DomainError, DomainResult, Frame, Roi};
use crate::infrastructure::capture::spout_ffi::*;
use std::ffi::CString;
use std::ptr;
use std::time::Instant;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::Interface;

pub struct SpoutCaptureAdapter {
    // FFIハンドル
    receiver: SpoutDxReceiverHandle,
    
    // DirectX 11 デバイス（自前で作成）
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    
    // 受信用テクスチャ（送信者のサイズに合わせて再作成）
    receive_tex: Option<ID3D11Texture2D>,
    receive_tex_size: (u32, u32),
    
    // ROI切り出し用ステージングテクスチャ
    staging_tex: Option<ID3D11Texture2D>,
    staging_size: (u32, u32),
    
    // 送信者情報
    sender_name: Option<String>,
    sender_info: SpoutDxSenderInfo,
    
    // デバイス情報（CapturePort用）
    device_info: DeviceInfo,
}

impl SpoutCaptureAdapter {
    /// 新しいSpoutキャプチャアダプタを作成
    ///
    /// # Arguments
    /// - `sender_name`: 接続する送信者名（Noneで自動選択）
    pub fn new(sender_name: Option<String>) -> DomainResult<Self> {
        // D3D11デバイスを作成
        let (device, context) = Self::create_d3d11_device()?;
        
        // Spoutレシーバーを作成
        let receiver = unsafe { spoutdx_receiver_create() };
        if receiver.is_null() {
            return Err(DomainError::Initialization(
                "Failed to create Spout receiver".to_string()
            ));
        }
        
        // D3D11デバイスをSpoutに渡す
        let device_ptr = unsafe { device.as_raw() as *mut std::ffi::c_void };
        let result = unsafe { spoutdx_receiver_open_dx11(receiver, device_ptr) };
        if !SpoutDxResult::from_raw(result).is_ok() {
            unsafe { spoutdx_receiver_destroy(receiver); }
            return Err(DomainError::Initialization(
                format!("Failed to open DX11 for Spout: {:?}", SpoutDxResult::from_raw(result))
            ));
        }
        
        // 送信者名を設定（指定があれば）
        if let Some(ref name) = sender_name {
            let c_name = CString::new(name.as_str())
                .map_err(|_| DomainError::Configuration("Invalid sender name".to_string()))?;
            unsafe { spoutdx_receiver_set_sender_name(receiver, c_name.as_ptr()); }
        } else {
            unsafe { spoutdx_receiver_set_sender_name(receiver, ptr::null()); }
        }
        
        // 初期デバイス情報（接続後に更新）
        let device_info = DeviceInfo {
            width: 0,
            height: 0,
            refresh_rate: 0,  // Spoutでは不明
            name: sender_name.clone().unwrap_or_else(|| "Spout (auto)".to_string()),
        };
        
        Ok(Self {
            receiver,
            device,
            context,
            receive_tex: None,
            receive_tex_size: (0, 0),
            staging_tex: None,
            staging_size: (0, 0),
            sender_name,
            sender_info: SpoutDxSenderInfo::default(),
            device_info,
        })
    }
    
    /// D3D11デバイスを作成
    fn create_d3d11_device() -> DomainResult<(ID3D11Device, ID3D11DeviceContext)> {
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        
        unsafe {
            D3D11CreateDevice(
                None,  // デフォルトアダプタ
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_FLAG(0),
                None,  // 機能レベル自動選択
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            ).map_err(|e| DomainError::Initialization(
                format!("Failed to create D3D11 device: {:?}", e)
            ))?;
        }
        
        let device = device.ok_or_else(|| 
            DomainError::Initialization("D3D11 device creation returned None".to_string())
        )?;
        let context = context.ok_or_else(|| 
            DomainError::Initialization("D3D11 context creation returned None".to_string())
        )?;
        
        Ok((device, context))
    }
    
    /// 受信テクスチャのサイズを更新（送信者変更時）
    fn update_receive_texture(&mut self, width: u32, height: u32, format: u32) -> DomainResult<()> {
        if self.receive_tex_size == (width, height) {
            return Ok(());  // サイズ変更なし
        }
        
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT(format as i32),
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: D3D11_RESOURCE_MISC_SHARED.0 as u32,
        };
        
        let mut tex: Option<ID3D11Texture2D> = None;
        unsafe {
            self.device.CreateTexture2D(&desc, None, Some(&mut tex))
                .map_err(|e| DomainError::Capture(
                    format!("Failed to create receive texture: {:?}", e)
                ))?;
        }
        
        self.receive_tex = tex;
        self.receive_tex_size = (width, height);
        
        // デバイス情報を更新
        self.device_info.width = width;
        self.device_info.height = height;
        
        Ok(())
    }
    
    /// ステージングテクスチャを確保（ROIサイズ用）
    fn ensure_staging_texture(&mut self, width: u32, height: u32) -> DomainResult<ID3D11Texture2D> {
        if let Some(ref tex) = self.staging_tex {
            if self.staging_size == (width, height) {
                return Ok(tex.clone());
            }
        }
        
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        
        let mut tex: Option<ID3D11Texture2D> = None;
        unsafe {
            self.device.CreateTexture2D(&desc, None, Some(&mut tex))
                .map_err(|e| DomainError::Capture(
                    format!("Failed to create staging texture: {:?}", e)
                ))?;
        }
        
        let tex = tex.ok_or_else(||
            DomainError::Capture("Staging texture creation returned None".to_string())
        )?;
        
        self.staging_tex = Some(tex.clone());
        self.staging_size = (width, height);
        
        Ok(tex)
    }
    
    /// ROIをクランプ
    fn clamp_roi(&self, roi: &Roi) -> Option<Roi> {
        let w = self.device_info.width;
        let h = self.device_info.height;
        
        if w == 0 || h == 0 || roi.width == 0 || roi.height == 0 {
            return None;
        }
        if roi.x >= w || roi.y >= h {
            return None;
        }
        
        let clamped_x = roi.x.min(w);
        let clamped_y = roi.y.min(h);
        let max_w = w.saturating_sub(clamped_x);
        let max_h = h.saturating_sub(clamped_y);
        let clamped_width = roi.width.min(max_w);
        let clamped_height = roi.height.min(max_h);
        
        if clamped_width == 0 || clamped_height == 0 {
            return None;
        }
        
        Some(Roi::new(clamped_x, clamped_y, clamped_width, clamped_height))
    }
}

impl CapturePort for SpoutCaptureAdapter {
    fn capture_frame_with_roi(&mut self, roi: &Roi) -> DomainResult<Option<Frame>> {
        // 送信者情報を取得
        let mut sender_info = SpoutDxSenderInfo::default();
        let result = unsafe { 
            spoutdx_receiver_get_sender_info(self.receiver, &mut sender_info) 
        };
        
        if !SpoutDxResult::from_raw(result).is_ok() {
            // 送信者未接続
            return Ok(None);
        }
        
        // 送信者のサイズが変わったらテクスチャを再作成
        if sender_info.width != self.sender_info.width 
           || sender_info.height != self.sender_info.height 
        {
            self.update_receive_texture(
                sender_info.width, 
                sender_info.height, 
                sender_info.format
            )?;
            self.sender_info = sender_info;
        }
        
        // 新しいフレームがあるかチェック
        let is_new = unsafe { spoutdx_receiver_is_frame_new(self.receiver) };
        if is_new == 0 {
            return Ok(None);  // 更新なし
        }
        
        // テクスチャを受信
        let receive_tex = self.receive_tex.as_ref()
            .ok_or_else(|| DomainError::Capture("Receive texture not initialized".to_string()))?;
        
        let tex_ptr = unsafe { receive_tex.as_raw() as *mut std::ffi::c_void };
        let result = unsafe { spoutdx_receiver_receive_texture(self.receiver, tex_ptr) };
        
        if !SpoutDxResult::from_raw(result).is_ok() {
            return Err(DomainError::Capture(
                format!("Failed to receive texture: {:?}", SpoutDxResult::from_raw(result))
            ));
        }
        
        // ROIのクランプ
        let clamped_roi = self.clamp_roi(roi).ok_or_else(|| {
            DomainError::Capture(format!(
                "ROI ({}, {}, {}x{}) is outside texture bounds ({}x{})",
                roi.x, roi.y, roi.width, roi.height,
                self.device_info.width, self.device_info.height
            ))
        })?;
        
        // ステージングテクスチャへROI領域をコピー
        let staging_tex = self.ensure_staging_texture(clamped_roi.width, clamped_roi.height)?;
        
        unsafe {
            let src_box = D3D11_BOX {
                left: clamped_roi.x,
                top: clamped_roi.y,
                front: 0,
                right: clamped_roi.x + clamped_roi.width,
                bottom: clamped_roi.y + clamped_roi.height,
                back: 1,
            };
            
            let src_resource: ID3D11Resource = receive_tex.clone().cast()
                .map_err(|e| DomainError::Capture(format!("Cast error: {:?}", e)))?;
            
            self.context.CopySubresourceRegion(
                &staging_tex,
                0, 0, 0, 0,
                &src_resource,
                0,
                Some(&src_box),
            );
        }
        
        // GPU→CPU転送
        let data_size = (clamped_roi.width * clamped_roi.height * 4) as usize;
        let mut data = vec![0u8; data_size];
        
        unsafe {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context.Map(
                &staging_tex,
                0,
                D3D11_MAP_READ,
                0,
                Some(&mut mapped),
            ).map_err(|e| DomainError::Capture(format!("Map failed: {:?}", e)))?;
            
            // RowPitchを考慮してコピー
            let src_ptr = mapped.pData as *const u8;
            let row_pitch = mapped.RowPitch as usize;
            let row_bytes = (clamped_roi.width * 4) as usize;
            
            for y in 0..clamped_roi.height as usize {
                let src_offset = y * row_pitch;
                let dst_offset = y * row_bytes;
                std::ptr::copy_nonoverlapping(
                    src_ptr.add(src_offset),
                    data.as_mut_ptr().add(dst_offset),
                    row_bytes,
                );
            }
            
            self.context.Unmap(&staging_tex, 0);
        }
        
        Ok(Some(Frame {
            data,
            width: clamped_roi.width,
            height: clamped_roi.height,
            timestamp: Instant::now(),
            dirty_rects: vec![],
        }))
    }
    
    fn reinitialize(&mut self) -> DomainResult<()> {
        // Spoutレシーバーを再作成
        unsafe {
            spoutdx_receiver_close_dx11(self.receiver);
            spoutdx_receiver_destroy(self.receiver);
        }
        
        // 新しいレシーバーを作成
        let receiver = unsafe { spoutdx_receiver_create() };
        if receiver.is_null() {
            return Err(DomainError::ReInitializationRequired);
        }
        
        let device_ptr = unsafe { self.device.as_raw() as *mut std::ffi::c_void };
        let result = unsafe { spoutdx_receiver_open_dx11(receiver, device_ptr) };
        if !SpoutDxResult::from_raw(result).is_ok() {
            unsafe { spoutdx_receiver_destroy(receiver); }
            return Err(DomainError::ReInitializationRequired);
        }
        
        // 送信者名を再設定
        if let Some(ref name) = self.sender_name {
            if let Ok(c_name) = CString::new(name.as_str()) {
                unsafe { spoutdx_receiver_set_sender_name(receiver, c_name.as_ptr()); }
            }
        } else {
            unsafe { spoutdx_receiver_set_sender_name(receiver, ptr::null()); }
        }
        
        self.receiver = receiver;
        self.receive_tex = None;
        self.receive_tex_size = (0, 0);
        self.staging_tex = None;
        self.staging_size = (0, 0);
        
        Ok(())
    }
    
    fn device_info(&self) -> DeviceInfo {
        self.device_info.clone()
    }
}

impl Drop for SpoutCaptureAdapter {
    fn drop(&mut self) {
        unsafe {
            spoutdx_receiver_close_dx11(self.receiver);
            spoutdx_receiver_destroy(self.receiver);
        }
    }
}
```

### Phase 3: 設定とDI

#### 3.1 config.toml の変更

```toml
[capture]
# キャプチャソース:
#   "dda"   - Desktop Duplication API（デフォルト）
#   "spout" - Spout DX11テクスチャ受信
source = "dda"

# Spout送信者名（source = "spout" の場合のみ有効）
# 空文字列または省略で最初のアクティブ送信者に自動接続
# spout_sender_name = "MyGame"

# 以下は既存設定...
timeout_ms = 8
```

#### 3.2 config.rs の追加

```rust
/// キャプチャソース
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureSource {
    /// Desktop Duplication API
    Dda,
    /// Spout DX11テクスチャ受信
    Spout,
}

impl Default for CaptureSource {
    fn default() -> Self {
        CaptureSource::Dda
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureConfig {
    /// キャプチャソース
    #[serde(default)]
    pub source: CaptureSource,
    
    /// Spout送信者名（source = "spout" の場合）
    #[serde(default)]
    pub spout_sender_name: Option<String>,
    
    // 既存フィールド...
    pub timeout_ms: u64,
    pub max_consecutive_timeouts: u32,
    // ...
}
```

#### 3.3 main.rs のDI変更

```rust
// キャプチャアダプタの初期化（設定に基づく選択）
let capture: Box<dyn CapturePort> = match config.capture.source {
    CaptureSource::Dda => {
        tracing::info!("Initializing DDA capture adapter...");
        Box::new(DdaCaptureAdapter::new(
            0,
            config.capture.monitor_index as usize,
            config.capture.timeout_ms as u32,
        )?)
    }
    CaptureSource::Spout => {
        tracing::info!("Initializing Spout capture adapter...");
        Box::new(SpoutCaptureAdapter::new(
            config.capture.spout_sender_name.clone(),
        )?)
    }
};
```

### Phase 4: ビルド設定

#### 4.1 build.rs の追加

```rust
// Spout DLL のコピー処理を追加
fn copy_spout_dlls(manifest_dir: &str, target_dir: &Path) {
    let spout_bin_dir = Path::new(manifest_dir)
        .join("third_party")
        .join("spoutdx-ffi")
        .join("bin");
    
    if !spout_bin_dir.exists() {
        println!("cargo:warning=Spout DLL directory not found: {}", spout_bin_dir.display());
        return;
    }
    
    // spoutdx_ffi.dll をコピー
    let dll_path = spout_bin_dir.join("spoutdx_ffi.dll");
    if dll_path.exists() {
        let dst_path = target_dir.join("spoutdx_ffi.dll");
        if let Err(e) = fs::copy(&dll_path, &dst_path) {
            println!("cargo:warning=Failed to copy Spout DLL: {}", e);
        }
    }
}

fn main() {
    // ... 既存のOpenCV DLLコピー ...
    
    // Spout DLLコピーを追加
    copy_spout_dlls(&manifest_dir, target_dir);
    
    // リンカー設定
    let spout_lib_dir = Path::new(&manifest_dir)
        .join("third_party")
        .join("spoutdx-ffi")
        .join("lib");
    println!("cargo:rustc-link-search=native={}", spout_lib_dir.display());
    println!("cargo:rerun-if-changed=third_party/spoutdx-ffi");
}
```

## エラーマッピング（Spout → Domain）

| SpoutDxResult | DomainError | 説明 |
|--------------|-------------|------|
| `SPOUTDX_OK` | - | 成功 |
| `SPOUTDX_ERROR_NOT_CONNECTED` | `Ok(None)` | 送信者未接続（正常扱い） |
| `SPOUTDX_ERROR_NULL_HANDLE` | `ReInitializationRequired` | ハンドル無効 |
| `SPOUTDX_ERROR_NULL_DEVICE` | `ReInitializationRequired` | デバイス無効 |
| `SPOUTDX_ERROR_INIT_FAILED` | `Initialization` | 初期化失敗 |
| `SPOUTDX_ERROR_RECEIVE_FAILED` | `DeviceNotAvailable` | 受信失敗（リトライ可能） |
| `SPOUTDX_ERROR_INTERNAL` | `Capture` | 内部エラー |

## DDAとSpoutの比較

| 項目 | DDA | Spout |
|------|-----|-------|
| ソース | 画面全体 | 特定アプリの送信テクスチャ |
| 遅延 | 1フレーム程度 | ゼロコピー（最小） |
| 解像度 | モニタ解像度 | 送信者設定に依存 |
| 権限 | 管理者権限が必要な場合あり | 不要 |
| 排他フルスクリーン | 対応（再初期化必要） | N/A（アプリ側対応次第） |
| フォーマット | BGRA固定 | 送信者設定に依存 |

## テスト戦略

### ユニットテスト

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spout_dx_result_conversion() {
        assert!(SpoutDxResult::from_raw(0).is_ok());
        assert_eq!(SpoutDxResult::from_raw(-3), SpoutDxResult::ErrorNotConnected);
    }

    #[test]
    fn test_sender_info_name_parsing() {
        let mut info = SpoutDxSenderInfo::default();
        info.name[0..4].copy_from_slice(&[b'T' as i8, b'e' as i8, b's' as i8, b't' as i8]);
        assert_eq!(info.name_as_string(), "Test");
    }
}
```

### 結合テスト（#[ignore]）

- Spout送信者が存在する環境でのみ実行
- 初期化、接続、フレーム受信の一連フロー
- 送信者切断時の再接続テスト

## 既知の制限

1. **同一アダプタ制約**: 送信側・受信側は同じGPUアダプタを使用する必要がある
2. **フォーマット制約**: 受信テクスチャのフォーマットは送信者に依存
3. **リフレッシュレート不明**: Spoutではリフレッシュレート情報が取得できない
4. **送信者依存**: 送信側アプリケーションがSpout対応していることが前提

## 参考リンク

- Spout2 GitHub: https://github.com/leadedge/Spout2
- Spout DirectX Texture Sharing: https://spout.zeal.co/
- DXGI Shared Textures: https://learn.microsoft.com/en-us/windows/win32/direct3d11/sharing-resources-between-processes

## 更新履歴

| 日付 | 内容 |
|------|------|
| 2026-01-08 | 初版作成（実装計画） |

````
