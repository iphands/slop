#version 460
#extension GL_EXT_ray_tracing : require
#extension GL_EXT_ray_tracing_position_fetch : require

layout(set = 0, binding = 0) uniform accelerationStructureEXT tlas;
layout(set = 0, binding = 3) uniform SceneUBO {
    vec3 lightPos;
    float lightIntensity;
    vec3 lightColor;
    float sphereCenterY;
    vec3 sphereCenter;
    float sphereRadius;
    int maxBounces;
} scene;

struct RayPayload {
    vec3 color;
    int depth;
};

layout(location = 0) rayPayloadInEXT RayPayload payload;
layout(location = 1) rayPayloadEXT float shadowPayload;
hitAttributeEXT vec2 attribs;

void main() {
    // Fetch triangle vertex positions
    vec3 v0 = gl_HitTriangleVertexPositionsEXT[0];
    vec3 v1 = gl_HitTriangleVertexPositionsEXT[1];
    vec3 v2 = gl_HitTriangleVertexPositionsEXT[2];

    // Compute flat geometric normal in world space
    vec3 edge1 = v1 - v0;
    vec3 edge2 = v2 - v0;
    vec3 normalObj = normalize(cross(edge1, edge2));
    vec3 normal = normalize(mat3(gl_ObjectToWorldEXT) * normalObj);

    // Ensure normal faces the camera
    if (dot(normal, gl_WorldRayDirectionEXT) > 0.0) {
        normal = -normal;
    }

    // Hit position
    vec3 hitPos = gl_WorldRayOriginEXT + gl_WorldRayDirectionEXT * gl_HitTEXT;

    // Color by primitive ID:
    // Primitives 0-1: Floor (white)
    // Primitives 2-3: Ceiling (white)
    // Primitives 4-5: Back wall (white)
    // Primitives 6-7: Left wall (red)
    // Primitives 8-9: Right wall (green)
    vec3 baseColor;
    int prim = gl_PrimitiveID;
    if (prim <= 5) {
        baseColor = vec3(0.73, 0.73, 0.73); // white walls
    } else if (prim <= 7) {
        baseColor = vec3(0.65, 0.05, 0.05); // red left wall
    } else {
        baseColor = vec3(0.12, 0.45, 0.15); // green right wall
    }

    // Point light shading
    vec3 toLight = scene.lightPos - hitPos;
    float lightDist = length(toLight);
    vec3 lightDir = toLight / lightDist;
    float diffuse = max(dot(normal, lightDir), 0.0);

    // Inverse square falloff
    float attenuation = scene.lightIntensity / (lightDist * lightDist + 0.01);

    // Shadow ray
    shadowPayload = 0.0;
    vec3 shadowOrigin = hitPos + normal * 0.001;
    traceRayEXT(
        tlas,
        gl_RayFlagsTerminateOnFirstHitEXT | gl_RayFlagsSkipClosestHitShaderEXT,
        0xFF,
        0, 0, 1, // sbtOffset=0, sbtStride=0, missIndex=1 (shadow miss)
        shadowOrigin,
        0.001,
        lightDir,
        lightDist - 0.001,
        1 // payload location 1
    );

    float shadow = shadowPayload;
    float ambient = 0.08;
    vec3 lighting = scene.lightColor * (ambient + shadow * diffuse * attenuation);
    vec3 diffuseColor = baseColor * lighting;

    // Metallic reflection
    const float metallic = 0.3; // 30% reflective
    if (payload.depth < scene.maxBounces && metallic > 0.0) {
        vec3 reflectDir = reflect(gl_WorldRayDirectionEXT, normal);
        vec3 reflectOrigin = hitPos + normal * 0.001;

        payload.color = vec3(0.0);
        payload.depth += 1;

        traceRayEXT(
            tlas,
            gl_RayFlagsNoneEXT,
            0xFF,
            0, 1, 0,
            reflectOrigin,
            0.001,
            reflectDir,
            10000.0,
            0
        );

        vec3 reflectedColor = payload.color;

        // Tint reflection by base color for colored metallic effect
        payload.color = mix(diffuseColor, reflectedColor * baseColor, metallic);
    } else {
        payload.color = diffuseColor;
    }
}
