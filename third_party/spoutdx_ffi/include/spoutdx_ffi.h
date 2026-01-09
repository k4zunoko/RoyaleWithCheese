#pragma once

#ifdef _WIN32
  #ifdef SPOUTDX_FFI_EXPORTS
    #define SPOUTDX_FFI_API __declspec(dllexport)
  #else
    #define SPOUTDX_FFI_API __declspec(dllimport)
  #endif
#else
  #define SPOUTDX_FFI_API
#endif

#ifdef __cplusplus
extern "C" {
#endif

// ============================================================
// Existing API
// ============================================================

// Pure C ABI for Spout DirectX functionality (Rust FFI ready)
SPOUTDX_FFI_API const char* spoutdx_ffi_version();

// Returns Spout SDK version number (e.g. 2007 for "2.007").
// Built from Spout source code - no DLL ABI issues.
SPOUTDX_FFI_API int spoutdx_ffi_get_sdk_version();

// Test DirectX 11 device creation (initialization check).
// Returns 1 on success, 0 on failure.
SPOUTDX_FFI_API int spoutdx_ffi_test_dx11_init();

// ============================================================
// Receiver API
// ============================================================

// -- Type definitions --

typedef void* SpoutDxReceiverHandle;

typedef enum SpoutDxResult {
    SPOUTDX_OK                   = 0,
    SPOUTDX_ERROR_NULL_HANDLE    = -1,
    SPOUTDX_ERROR_NULL_DEVICE    = -2,
    SPOUTDX_ERROR_NOT_CONNECTED  = -3,
    SPOUTDX_ERROR_INIT_FAILED    = -4,
    SPOUTDX_ERROR_RECEIVE_FAILED = -5,
    SPOUTDX_ERROR_INTERNAL       = -99
} SpoutDxResult;

typedef struct SpoutDxSenderInfo {
    char name[256];
    unsigned int width;
    unsigned int height;
    unsigned int format;  // DXGI_FORMAT
} SpoutDxSenderInfo;

// -- Lifecycle --

SPOUTDX_FFI_API SpoutDxReceiverHandle spoutdx_receiver_create(void);
SPOUTDX_FFI_API int spoutdx_receiver_destroy(SpoutDxReceiverHandle handle);

// -- DirectX initialization --

SPOUTDX_FFI_API int spoutdx_receiver_open_dx11(
    SpoutDxReceiverHandle handle,
    void* device  // ID3D11Device*
);
SPOUTDX_FFI_API int spoutdx_receiver_close_dx11(SpoutDxReceiverHandle handle);

// -- Receive configuration --

SPOUTDX_FFI_API int spoutdx_receiver_set_sender_name(
    SpoutDxReceiverHandle handle,
    const char* sender_name  // NULL for active sender
);

// -- Receive --

// Receive to user-provided texture
SPOUTDX_FFI_API int spoutdx_receiver_receive_texture(
    SpoutDxReceiverHandle handle,
    void* dst_texture  // ID3D11Texture2D*
);

// Receive to internal class texture (no copy needed)
// After this, use spoutdx_receiver_get_received_texture() to get the texture
SPOUTDX_FFI_API int spoutdx_receiver_receive(SpoutDxReceiverHandle handle);

// Get the internally received texture (valid after spoutdx_receiver_receive)
// Returns: ID3D11Texture2D* or NULL
SPOUTDX_FFI_API void* spoutdx_receiver_get_received_texture(SpoutDxReceiverHandle handle);

// Get the D3D11 context used by SpoutDX (for CopyResource, etc.)
// Returns: ID3D11DeviceContext* or NULL
SPOUTDX_FFI_API void* spoutdx_receiver_get_dx11_context(SpoutDxReceiverHandle handle);

SPOUTDX_FFI_API int spoutdx_receiver_release(SpoutDxReceiverHandle handle);

// -- State query --

SPOUTDX_FFI_API int spoutdx_receiver_get_sender_info(
    SpoutDxReceiverHandle handle,
    SpoutDxSenderInfo* out_info
);

SPOUTDX_FFI_API int spoutdx_receiver_is_updated(SpoutDxReceiverHandle handle);
SPOUTDX_FFI_API int spoutdx_receiver_is_connected(SpoutDxReceiverHandle handle);
SPOUTDX_FFI_API int spoutdx_receiver_is_frame_new(SpoutDxReceiverHandle handle);

#ifdef __cplusplus
}
#endif
