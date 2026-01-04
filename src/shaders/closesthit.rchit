#version 460
#extension GL_EXT_ray_tracing : require
#extension GL_EXT_nonuniform_qualifier : enable
#extension GL_EXT_scalar_block_layout : enable
#extension GL_EXT_shader_explicit_arithmetic_types_int64 : require
#extension GL_EXT_buffer_reference2 : require

layout(binding = 0, set = 0) uniform accelerationStructureEXT topLevelAS;
layout(binding = 2, set = 0) uniform CameraProperties {
    mat4 viewInverse;
    mat4 projInverse;
    vec4 lightPos;
    vec4 settings; // x: soft_shadows, y: reflections, z: refraction, w: sss
} cam;

struct SceneDesc {
    uint64_t vertexAddress;
    uint64_t indexAddress;
    uint64_t materialAddress; // Points to array of materials, we use gl_InstanceCustomIndexEXT to index into it? No, usually material ID is per instance.
    // Let's assume materialAddress points to the start of the Materials buffer.
    // And we have a way to know which material index to use. 
    // For simplicity, let's put materialIndex in the Instance Custom Index.
};

layout(binding = 3, set = 0) buffer SceneDesc_ { SceneDesc sceneDesc[]; };

struct Vertex {
    float pos[3];
    float nrm[3];
    float color[3];
};

struct Material {
    vec4 color;
    vec4 params; // x: type, y: roughness, z: ior, w: sss_amount
};

layout(buffer_reference, scalar) buffer Vertices { Vertex v[]; };
layout(buffer_reference, scalar) buffer Indices { uvec3 i[]; };
layout(buffer_reference, scalar) buffer Materials { Material m[]; };

struct RayPayload {
    vec3 color;
    uint depth;
    uint seed;
};

layout(location = 0) rayPayloadInEXT RayPayload prd;
layout(location = 1) rayPayloadEXT bool isShadowed;

// Random
uint tea(uint val0, uint val1) {
  uint v0 = val0;
  uint v1 = val1;
  uint s0 = 0;

  for(uint n = 0; n < 16; n++) {
    s0 += 0x9e3779b9;
    v0 += ((v1 << 4) + 0xa341316c) ^ (v1 + s0) ^ ((v1 >> 5) + 0xc8013ea4);
    v1 += ((v0 << 4) + 0xad90777d) ^ (v0 + s0) ^ ((v0 >> 5) + 0x7e95761e);
  }
  return v0;
}

float rnd(inout uint prev) {
  prev = (prev * 8121 + 28411) % 65535;
  return float(prev) / 65535.0;
}

void main() {
    // Get Geometry
    SceneDesc desc = sceneDesc[gl_InstanceID];
    Vertices vertices = Vertices(desc.vertexAddress);
    Indices indices = Indices(desc.indexAddress);
    Materials materials = Materials(desc.materialAddress);

    uvec3 ind = indices.i[gl_PrimitiveID];
    
    Vertex v0 = vertices.v[ind.x];
    Vertex v1 = vertices.v[ind.y];
    Vertex v2 = vertices.v[ind.z];

    const vec3 barycentrics = vec3(1.0 - gl_HitTEXT.x - gl_HitTEXT.y, gl_HitTEXT.x, gl_HitTEXT.y);

    vec3 n0 = vec3(v0.nrm[0], v0.nrm[1], v0.nrm[2]);
    vec3 n1 = vec3(v1.nrm[0], v1.nrm[1], v1.nrm[2]);
    vec3 n2 = vec3(v2.nrm[0], v2.nrm[1], v2.nrm[2]);
    vec3 normal = normalize(n0 * barycentrics.x + n1 * barycentrics.y + n2 * barycentrics.z);
    
    // Transform normal to world space
    normal = normalize(vec3(gl_ObjectToWorldEXT * vec4(normal, 0.0)));
    vec3 worldPos = gl_WorldRayOriginEXT + gl_WorldRayDirectionEXT * gl_HitTEXT;

    // Material
    int matIndex = gl_InstanceCustomIndexEXT;
    Material mat = materials.m[matIndex];
    vec3 albedo = mat.color.rgb;
    float type = mat.params.x; // 0: Lambert, 1: Metal, 2: Glass, 3: SSS, ...
    float roughness = mat.params.y;
    float ior = mat.params.z;

    vec3 lightDir = normalize(cam.lightPos.xyz - worldPos);
    float distToLight = length(cam.lightPos.xyz - worldPos);

    // Soft Shadow (jitter light pos)
    if (cam.settings.x > 0.0) {
        float r1 = rnd(prd.seed);
        float r2 = rnd(prd.seed);
        vec3 offset = vec3(r1 - 0.5, r2 - 0.5, (r1+r2) - 1.0) * 1.0; // Simple jitter
        lightDir = normalize((cam.lightPos.xyz + offset) - worldPos);
    }

    // Shadow Ray
    isShadowed = true;
    uint rayFlags = gl_RayFlagsTerminateOnFirstHitEXT | gl_RayFlagsOpaqueEXT | gl_RayFlagsSkipClosestHitShaderEXT;
    traceRayEXT(topLevelAS, rayFlags, 0xff, 0, 0, 1, worldPos, 0.01, lightDir, distToLight, 1);

    vec3 lighting = vec3(0.0);
    if (!isShadowed) {
        float NdotL = max(dot(normal, lightDir), 0.0);
        lighting += albedo * NdotL;
    } else {
        lighting += albedo * 0.1; // Ambient
    }

    // Reflection / Refraction (Simplified)
    if (prd.depth < 5) {
        if (type == 1.0 && cam.settings.y > 0.0) { // Metal
             vec3 refDir = reflect(gl_WorldRayDirectionEXT, normal);
             prd.depth++;
             traceRayEXT(topLevelAS, gl_RayFlagsOpaqueEXT, 0xff, 0, 0, 0, worldPos, 0.01, refDir, 1000.0, 0);
             lighting = mix(lighting, prd.color, 1.0 - roughness);
        }
        else if (type == 2.0 && cam.settings.z > 0.0) { // Glass
             float eta = 1.0 / ior;
             if (dot(gl_WorldRayDirectionEXT, normal) > 0) {
                 normal = -normal;
                 eta = ior;
             }
             vec3 refDir = refract(gl_WorldRayDirectionEXT, normal, eta);
             if (length(refDir) > 0.0) {
                 prd.depth++;
                 traceRayEXT(topLevelAS, gl_RayFlagsOpaqueEXT, 0xff, 0, 0, 0, worldPos, 0.01, refDir, 1000.0, 0);
                 lighting = mix(lighting, prd.color, 0.9);
             } else {
                 // TIR -> Reflect
                 vec3 rDir = reflect(gl_WorldRayDirectionEXT, normal);
                 prd.depth++;
                 traceRayEXT(topLevelAS, gl_RayFlagsOpaqueEXT, 0xff, 0, 0, 0, worldPos, 0.01, rDir, 1000.0, 0);
                 lighting = mix(lighting, prd.color, 0.9);
             }
        }
    }
    
    // SSS (Very Fake)
    if (type == 3.0 && cam.settings.w > 0.0) {
        // Wrap lighting
        float wrap = 0.5;
        float NdotL = max(dot(normal, lightDir) + wrap, 0.0) / (1.0 + wrap);
        lighting = albedo * NdotL + vec3(0.1, 0.0, 0.0); // Subsurface tint
    }

    prd.color = lighting;
}
