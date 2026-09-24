#version 450
// VERTEX SHADER - Base PBR Vertex Shader

// This shader transforms mesh vertices from model space to clip space.
// It supports instanced rendering and optional frustum culling.

// Per-instance data structure - one per draw instance
// Layout matches the GPU buffer layout for InstanceData
struct InstanceData {
    mat4 model;           // Model-to-world transformation matrix
    vec4 color;           // xyz = base color, w = emissive intensity
    vec4 mat_props;      // x = roughness, y = metalness, z = unused, w = unused
    vec4 velocity;       // xyz = linear velocity, w = bounciness
    vec4 physic;          // x = collision type sort key, y = mass, z = gravity scale, w = unused
    vec4 rotation;       // x = angular velocity X, y = Y, z = Z, w = friction
};

// INPUT ATTRIBUTES (from vertex buffers)

// Vertex attribute layout must match the mesh's vertex format
layout(location = 0) in vec3 position;   // Vertex position in model space
layout(location = 1) in vec3 normal;   // Vertex normal in model space  
layout(location = 2) in vec2 uv;       // Texture coordinates

// OUTPUT VARYINGS (to fragment shader)

layout(location = 0) out vec3 v_color;          // Base color RGB
layout(location = 1) out vec3 v_normal;        // World-space normal (for lighting)
layout(location = 2) out vec3 v_pos;        // World-space position (for lighting)
layout(location = 3) out vec4 v_mat_data;     // Material properties (roughness, metalness)
layout(location = 4) out vec2 v_uv;        // Texture coordinates
layout(location = 5) out float v_emissive;  // Emissive strength (1.0 = glowing)
layout(location = 6) out vec2 v_screen_uv;  // NDC coordinates for post-processing

// UNIFORM BUFFER - Camera and scene data

layout(set = 0, binding = 0) uniform UniformBufferObject {
    mat4 view;       // View matrix (world to camera)
    mat4 proj;       // Projection matrix (camera to clip)
    vec3 eye_pos;   // Camera position in world space
    uint v_base_instance;  // Base instance ID (unused here)
} ubo;

// PUSH CONSTANTS - Per-draw parameters

layout(push_constant) uniform MeshPush {
    uint v_visible_list_offset;   // Offset into visible list for indirect rendering
    uint v_use_culling;        // 0 = draw all, 1 = cull with visible list
} mesh_pc;

// STORAGE BUFFERS - Instance data

// Read-only buffer containing all instance transforms and properties
layout(std430, set = 0, binding = 2) readonly buffer InstanceBuffer { 
    InstanceData instances[]; 
};

// Read-only buffer containing visible instance indices (filled by cull shader)
layout(std430, set = 0, binding = 3) readonly buffer VisibleIndices { 
    uint data[]; 
};

// MAIN ENTRY POINT

void main() {
    // Determine which instance to render:
    // If culling is enabled, read from visible list.
    // Otherwise, use instance ID directly.
    uint actual_id;
    if (mesh_pc.v_use_culling == 1) {
        // Read instance index from the culling output buffer
        actual_id = data[mesh_pc.v_visible_list_offset + gl_InstanceIndex];
    } else {
        // Direct instance indexing (no culling)
        actual_id = gl_InstanceIndex;
    }
    
    // Fetch this instance's data from the buffer
    InstanceData inst = instances[actual_id];

    // Transform vertex position from model space to world space
    vec4 world_pos = inst.model * vec4(position, 1.0);
    
    // Transform to clip space for rasterization
    gl_Position = ubo.proj * ubo.view * world_pos;

    // Pass world-space position to fragment shader for lighting
    v_pos = world_pos.xyz;
    
    // Pass color and emissive to fragment shader
    v_color = inst.color.xyz;
    v_emissive = inst.color.w;

    // Transform normal to world space using normal matrix
    // The normal matrix is the inverse-transpose of the model matrix's upper 3x3
    mat3 normal_matrix = transpose(inverse(mat3(inst.model)));
    v_normal = normal_matrix * normal;

    // Pass material properties for PBR shading
    v_mat_data = inst.mat_props;
    
    // Pass texture coordinates
    v_uv = uv;
    
    // Calculate screen-space UV for post-processing (vignette, etc.)
    v_screen_uv = (gl_Position.xy / gl_Position.w) * 0.5 + 0.5;
}