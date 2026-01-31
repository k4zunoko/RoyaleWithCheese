#!/usr/bin/env bash
# GPU vs CPU パフォーマンス比較スクリプト
#
# 使用方法:
#   ./scripts/benchmark_gpu_cpu.sh [duration_seconds]
#
# 例:
#   ./scripts/benchmark_gpu_cpu.sh 30  # 30秒間ベンチマーク

set -e

DURATION=${1:-30}  # デフォルト30秒
RESULTS_DIR="benchmark_results"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)

echo "======================================"
echo "GPU vs CPU パフォーマンス比較"
echo "======================================"
echo ""
echo "測定時間: ${DURATION}秒"
echo "結果保存先: ${RESULTS_DIR}/"
echo ""

# 結果ディレクトリ作成
mkdir -p "${RESULTS_DIR}"

# ログファイルパス
CPU_LOG="${RESULTS_DIR}/cpu_${TIMESTAMP}.log"
GPU_LOG="${RESULTS_DIR}/gpu_${TIMESTAMP}.log"

echo "Step 1: CPU版のベンチマーク"
echo "------------------------------"
echo "config.tomlをCPU設定に変更..."

# CPU設定のconfig.tomlを作成
cat > config_benchmark_cpu.toml << 'EOF'
[capture]
source = "wgc"
timeout_ms = 8
monitor_index = 0
max_consecutive_timeouts = 120
reinit_initial_delay_ms = 100
reinit_max_delay_ms = 5000

[process]
mode = "fast-color"
detection_method = "moments"
min_detection_area = 100

[process.roi]
width = 800
height = 600

[process.hsv_range]
h_min = 20
h_max = 40
s_min = 100
s_max = 255
v_min = 100
v_max = 255

[gpu]
enabled = false
device_index = 0
prefer_gpu = false

[pipeline]
enable_dirty_rect_optimization = true
stats_interval_sec = 5

[communication]
vendor_id = 0x0000
product_id = 0x0000
hid_send_interval_ms = 8

[activation]
max_distance_from_center = 50.0
active_window_ms = 500

[audio_feedback]
enabled = false
EOF

echo "CPU版を実行中... ((${DURATION}秒)"
echo "ログ: ${CPU_LOG}"
timeout ${DURATION} cargo run --features performance-timing -- --config config_benchmark_cpu.toml 2>&1 | tee "${CPU_LOG}" || true

echo ""
echo "Step 2: GPU版のベンチマーク"
echo "------------------------------"
echo "config.tomlをGPU設定に変更..."

# GPU設定のconfig.tomlを作成
cat > config_benchmark_gpu.toml << 'EOF'
[capture]
source = "wgc"
timeout_ms = 8
monitor_index = 0
max_consecutive_timeouts = 120
reinit_initial_delay_ms = 100
reinit_max_delay_ms = 5000

[process]
mode = "fast-color"
detection_method = "moments"
min_detection_area = 100

[process.roi]
width = 800
height = 600

[process.hsv_range]
h_min = 20
h_max = 40
s_min = 100
s_max = 255
v_min = 100
v_max = 255

[gpu]
enabled = true
device_index = 0
prefer_gpu = true

[pipeline]
enable_dirty_rect_optimization = true
stats_interval_sec = 5

[communication]
vendor_id = 0x0000
product_id = 0x0000
hid_send_interval_ms = 8

[activation]
max_distance_from_center = 50.0
active_window_ms = 500

[audio_feedback]
enabled = false
EOF

echo "GPU版を実行中... ((${DURATION}秒)"
echo "ログ: ${GPU_LOG}"
timeout ${DURATION} cargo run --features performance-timing -- --config config_benchmark_gpu.toml 2>&1 | tee "${GPU_LOG}" || true

echo ""
echo "Step 3: 結果分析"
echo "------------------------------"

# 結果を解析
echo ""
echo "【CPU版 結果】"
echo "--------------"
grep -E "(WGC CPU Capture:|Process:|EndToEnd:)" "${CPU_LOG}" | tail -10 || echo "(データなし)"

echo ""
echo "【GPU版 結果】"
echo "--------------"
grep -E "(WGC GPU Capture:|Process:|EndToEnd:)" "${GPU_LOG}" | tail -10 || echo "(データなし)"

echo ""
echo "Step 4: 統計情報"
echo "------------------------------"

# Process時間の平均を計算
echo ""
echo "Process時間 (p50/p95/p99):"
echo "CPU:"
grep "Process:" "${CPU_LOG}" | grep -oE "p50=[0-9.]+ms, p95=[0-9.]+ms, p99=[0-9.]+ms" | tail -1 || echo "(データなし)"

echo "GPU:"
grep "Process:" "${GPU_LOG}" | grep -oE "p50=[0-9.]+ms, p95=[0-9.]+ms, p99=[0-9.]+ms" | tail -1 || echo "(データなし)"

echo ""
echo "EndToEnd時間 (p50/p95/p99):"
echo "CPU:"
grep "EndToEnd:" "${CPU_LOG}" | grep -oE "p50=[0-9.]+ms, p95=[0-9.]+ms, p99=[0-9.]+ms" | tail -1 || echo "(データなし)"

echo "GPU:"
grep "EndToEnd:" "${GPU_LOG}" | grep -oE "p50=[0-9.]+ms, p95=[0-9.]+ms, p99=[0-9.]+ms" | tail -1 || echo "(データなし)"

echo ""
echo "======================================"
echo "ベンチマーク完了"
echo "======================================"
echo ""
echo "詳細なログ:"
echo "  CPU: ${CPU_LOG}"
echo "  GPU: ${GPU_LOG}"
echo ""
echo "注意:"
echo "  - 結果はシステムの状態により変動します"
echo "  - 複数回実行して平均を取ることを推奨"
echo "  - GPU版はGPUの種類により大きく変動"

# 一時ファイル削除
rm -f config_benchmark_cpu.toml config_benchmark_gpu.toml
