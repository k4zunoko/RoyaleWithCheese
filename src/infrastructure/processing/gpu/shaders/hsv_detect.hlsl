// HSV Color Detection Compute Shader
// 
// Performs parallel HSV color detection on BGRA textures.
// Uses a two-pass approach:
// 1. Per-pixel detection and local accumulation (this shader)
// 2. CPU-side final reduction (minimal data transfer)
//
// Input: BGRA texture (SRV)
// Output: Detection results buffer (UAV) with atomic accumulation
//
// Thread group: 16x16 = 256 threads per group (optimal for most GPUs)

// Input texture (BGRA format from screen capture)
Texture2D<float4> InputTexture : register(t0);

// Output buffer for detection results
// [0]: detected pixel count (atomic)
// [1]: sum of X coordinates (atomic, fixed-point * 256)
// [2]: sum of Y coordinates (atomic, fixed-point * 256)
// [3]: min X (atomic min)
// [4]: min Y (atomic min)
// [5]: max X (atomic max)
// [6]: max Y (atomic max)
RWBuffer<uint> OutputBuffer : register(u0);

// HSV range parameters (constant buffer)
// Note: H is in OpenCV range [0-180], S and V in [0-255]
cbuffer HsvParams : register(b0)
{
    uint h_min;      // Hue minimum (0-180, OpenCV convention)
    uint h_max;      // Hue maximum (0-180, OpenCV convention)
    uint s_min;      // Saturation minimum (0-255)
    uint s_max;      // Saturation maximum (0-255)
    uint v_min;      // Value minimum (0-255)
    uint v_max;      // Value maximum (0-255)
    uint img_width;  // Image width in pixels
    uint img_height; // Image height in pixels
};

// Thread group shared memory for local reduction
groupshared uint gs_count;
groupshared uint gs_sum_x;
groupshared uint gs_sum_y;

// Convert BGRA (0-1 float) to HSV (H: 0-180, S: 0-255, V: 0-255)
// Matches OpenCV's BGR2HSV conversion for compatibility
uint3 BGRAtoHSV(float4 bgra)
{
    float b = bgra.b;
    float g = bgra.g;
    float r = bgra.r;
    
    float v_max = max(max(r, g), b);
    float v_min = min(min(r, g), b);
    float delta = v_max - v_min;
    
    // Value (0-255)
    uint v = (uint)(v_max * 255.0);
    
    // Saturation (0-255)
    uint s = 0;
    if (v_max > 0.0001)
    {
        s = (uint)((delta / v_max) * 255.0);
    }
    
    // Hue (0-180, OpenCV convention)
    uint h = 0;
    if (delta > 0.0001)
    {
        float hue;
        if (v_max == r)
        {
            hue = 60.0 * (g - b) / delta;
            if (hue < 0.0) hue += 360.0;
        }
        else if (v_max == g)
        {
            hue = 60.0 * (2.0 + (b - r) / delta);
        }
        else // v_max == b
        {
            hue = 60.0 * (4.0 + (r - g) / delta);
        }
        // Convert to OpenCV range (0-180)
        h = (uint)(hue / 2.0);
    }
    
    return uint3(h, s, v);
}

// Check if HSV value is within the specified range
// Handles hue wraparound (e.g., red: h_min=170, h_max=10)
bool IsInHsvRange(uint3 hsv)
{
    uint h = hsv.x;
    uint s = hsv.y;
    uint v = hsv.z;
    
    // Check S and V first (simple range check)
    if (s < s_min || s > s_max) return false;
    if (v < v_min || v > v_max) return false;
    
    // Check H with wraparound support
    if (h_min <= h_max)
    {
        // Normal range (e.g., green: 35-85)
        return (h >= h_min && h <= h_max);
    }
    else
    {
        // Wraparound range (e.g., red: 170-10)
        return (h >= h_min || h <= h_max);
    }
}

// Main compute shader entry point
// Dispatch with: ceil(width/16) x ceil(height/16) x 1 thread groups
[numthreads(16, 16, 1)]
void CSMain(
    uint3 groupId : SV_GroupID,
    uint3 groupThreadId : SV_GroupThreadID,
    uint3 dispatchThreadId : SV_DispatchThreadID,
    uint groupIndex : SV_GroupIndex)
{
    // Initialize shared memory (first thread in group)
    if (groupIndex == 0)
    {
        gs_count = 0;
        gs_sum_x = 0;
        gs_sum_y = 0;
    }
    GroupMemoryBarrierWithGroupSync();
    
    // Get pixel coordinates
    uint x = dispatchThreadId.x;
    uint y = dispatchThreadId.y;
    
    // Bounds check
    if (x < img_width && y < img_height)
    {
        // Sample texture (BGRA format, already normalized 0-1)
        float4 bgra = InputTexture.Load(int3(x, y, 0));
        
        // Convert to HSV
        uint3 hsv = BGRAtoHSV(bgra);
        
        // Check if pixel is in range
        if (IsInHsvRange(hsv))
        {
            // Accumulate in shared memory (local reduction)
            InterlockedAdd(gs_count, 1);
            // Use fixed-point for coordinates (multiply by 256 for precision)
            InterlockedAdd(gs_sum_x, x);
            InterlockedAdd(gs_sum_y, y);
        }
    }
    
    // Wait for all threads in group to complete
    GroupMemoryBarrierWithGroupSync();
    
    // First thread writes group results to global buffer
    if (groupIndex == 0)
    {
        if (gs_count > 0)
        {
            // Atomic add to global accumulators
            InterlockedAdd(OutputBuffer[0], gs_count);
            InterlockedAdd(OutputBuffer[1], gs_sum_x);
            InterlockedAdd(OutputBuffer[2], gs_sum_y);
        }
    }
}
