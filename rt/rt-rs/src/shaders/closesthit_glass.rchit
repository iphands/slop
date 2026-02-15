#version 460
#extension GL_EXT_ray_tracing : require

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
hitAttributeEXT vec3 hitNormal;
const float IOR_GLASS = 1.5;

float fresnelSchlick(float cosTheta, float iorIn, float iorOut) {
    float r0 = (iorIn - iorOut) / (iorIn + iorOut);
    r0 = r0 * r0;
    return r0 + (1.0 - r0) * pow(1.0 - cosTheta, 5.0);
}

void main() {
    if (payload.depth >= scene.maxBounces) {
        payload.color = vec3(0.0);
        return;
    }

    vec3 hitPos = gl_WorldRayOriginEXT + gl_WorldRayDirectionEXT * gl_HitTEXT;
    vec3 normal = normalize(hitNormal);
    vec3 incident = normalize(gl_WorldRayDirectionEXT);

    // Determine if entering or exiting glass
    float cosI = dot(-incident, normal);
    float etaIn, etaOut;
    vec3 n;
    if (cosI > 0.0) {
        // Entering glass
        n = normal;
        etaIn = 1.0;
        etaOut = IOR_GLASS;
    } else {
        // Exiting glass
        n = -normal;
        cosI = -cosI;
        etaIn = IOR_GLASS;
        etaOut = 1.0;
    }

    float eta = etaIn / etaOut;
    float fresnel = fresnelSchlick(cosI, etaIn, etaOut);

    // Check for total internal reflection
    float sinT2 = eta * eta * (1.0 - cosI * cosI);
    bool totalInternalReflection = sinT2 > 1.0;

    vec3 reflectedColor = vec3(0.0);
    vec3 refractedColor = vec3(0.0);

    int nextDepth = payload.depth + 1;
    vec3 origin = hitPos;

    // Reflection ray
    {
        vec3 reflDir = reflect(incident, n);
        payload.color = vec3(0.0);
        payload.depth = nextDepth;
        traceRayEXT(
            tlas,
            gl_RayFlagsNoneEXT,
            0xFF,
            0, 1, 0,
            origin + reflDir * 0.001,
            0.001,
            reflDir,
            10000.0,
            0
        );
        reflectedColor = payload.color;
    }

    if (!totalInternalReflection) {
        // Refraction ray
        vec3 refrDir = refract(incident, n, eta);
        payload.color = vec3(0.0);
        payload.depth = nextDepth;
        traceRayEXT(
            tlas,
            gl_RayFlagsNoneEXT,
            0xFF,
            0, 1, 0,
            origin + refrDir * 0.001,
            0.001,
            refrDir,
            10000.0,
            0
        );
        refractedColor = payload.color;
    }

    if (totalInternalReflection) {
        payload.color = reflectedColor;
    } else {
        payload.color = mix(refractedColor, reflectedColor, fresnel);
    }
}
