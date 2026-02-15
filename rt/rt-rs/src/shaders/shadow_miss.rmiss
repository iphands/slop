#version 460
#extension GL_EXT_ray_tracing : require

// Shadow ray payload at location 1
layout(location = 1) rayPayloadInEXT float shadowPayload;

void main() {
    // Miss means NOT in shadow
    shadowPayload = 1.0;
}
