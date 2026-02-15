#version 460
#extension GL_EXT_ray_tracing : require

struct RayPayload {
    vec3 color;
    int depth;
};

layout(location = 0) rayPayloadInEXT RayPayload payload;

void main() {
    // Black background - we're inside a closed Cornell box
    payload.color = vec3(0.0, 0.0, 0.0);
}
