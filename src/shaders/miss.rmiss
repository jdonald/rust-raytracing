#version 460
#extension GL_EXT_ray_tracing : require

struct RayPayload {
    vec3 color;
    uint depth;
    uint seed;
};

layout(location = 0) rayPayloadInEXT RayPayload prd;

void main() {
    // Simple gradient sky
    vec3 unitDir = normalize(gl_WorldRayDirectionEXT);
    float t = 0.5 * (unitDir.y + 1.0);
    prd.color = mix(vec3(1.0, 1.0, 1.0), vec3(0.5, 0.7, 1.0), t);
}
