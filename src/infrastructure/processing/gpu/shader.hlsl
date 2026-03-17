Texture2D<float4> InputTexture : register(t0);
RWBuffer<uint> OutputBuffer : register(u0);

cbuffer HsvParams : register(b0)
{
    uint h_low;
    uint h_high;
    uint s_low;
    uint s_high;
    uint v_low;
    uint v_high;
    uint width;
    uint height;
};

uint3 BGRAtoHSV(float4 bgra)
{
    float b = bgra.b;
    float g = bgra.g;
    float r = bgra.r;

    float maxc = max(max(r, g), b);
    float minc = min(min(r, g), b);
    float delta = maxc - minc;

    uint v = (uint)(maxc * 255.0);
    uint s = 0;
    uint h = 0;

    if (maxc > 0.0001)
    {
        s = (uint)((delta / maxc) * 255.0);
    }

    if (delta > 0.0001)
    {
        float hue;
        if (maxc == r)
        {
            hue = 60.0 * (g - b) / delta;
            if (hue < 0.0) hue += 360.0;
        }
        else if (maxc == g)
        {
            hue = 60.0 * (2.0 + (b - r) / delta);
        }
        else
        {
            hue = 60.0 * (4.0 + (r - g) / delta);
        }
        h = (uint)(hue / 2.0);
    }

    return uint3(h, s, v);
}

bool IsInHsvRange(uint3 hsv)
{
    uint h = hsv.x;
    uint s = hsv.y;
    uint v = hsv.z;

    if (s < s_low || s > s_high) return false;
    if (v < v_low || v > v_high) return false;

    if (h_low <= h_high)
    {
        return h >= h_low && h <= h_high;
    }
    return h >= h_low || h <= h_high;
}

[numthreads(16, 16, 1)]
void CSMain(uint3 dispatchThreadId : SV_DispatchThreadID)
{
    uint x = dispatchThreadId.x;
    uint y = dispatchThreadId.y;

    if (x >= width || y >= height)
    {
        return;
    }

    float4 bgra = InputTexture.Load(int3(x, y, 0));
    uint3 hsv = BGRAtoHSV(bgra);

    if (IsInHsvRange(hsv))
    {
        InterlockedAdd(OutputBuffer[0], 1);
        InterlockedAdd(OutputBuffer[1], x);
        InterlockedAdd(OutputBuffer[2], y);
    }
}
