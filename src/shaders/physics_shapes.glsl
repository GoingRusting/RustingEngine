// Collider shapes shared by the built-in physics shader and the GPU
// body-contact passes. Not part of the custom-shader ABI.

// Shape encoding: x = kind (0 box, 1 sphere, 2 capsule along local Y,
// 3 none); box yzw = half extents; sphere y = radius; capsule y = half
// height, z = radius. Material x = friction, y = restitution. Layers x =
// memberships, y = filters.
struct BodyShape {
    vec4 shape;
    vec4 material;
    uvec4 layers;
};
layout(set = 0, binding = 6) readonly buffer BodyShapes { BodyShape data[]; } body_shapes;

// How far a shape reaches from its center along world direction `n`.
float support(vec4 shape, mat3 rotation, vec3 n) {
    vec3 local = transpose(rotation) * n;
    if (shape.x < 0.5) return dot(abs(local), shape.yzw);
    if (shape.x < 1.5) return shape.y;
    return shape.z + abs(local.y) * shape.y;
}

// Radius of the sphere around the center that holds the whole shape.
float bounding_radius(vec4 shape) {
    if (shape.x < 0.5) return length(shape.yzw);
    if (shape.x < 1.5) return shape.y;
    return shape.y + shape.z;
}

// Rotation of a model matrix with its scale removed.
mat3 rotation_of(mat4 model) {
    return mat3(normalize(model[0].xyz), normalize(model[1].xyz), normalize(model[2].xyz));
}

// Direction from the surface of `shape` (centered at `origin`, turned by
// `rotation`) to `point`, and the signed distance (negative inside).
void shape_distance(vec4 shape, mat3 rotation, vec3 origin, vec3 point, out vec3 normal, out float distance) {
    vec3 local = transpose(rotation) * (point - origin);
    vec3 local_normal = vec3(0.0, 1.0, 0.0);
    if (shape.x < 0.5) {
        vec3 half_extents = shape.yzw;
        vec3 outside = abs(local) - half_extents;
        float largest = max(outside.x, max(outside.y, outside.z));
        if (largest > 0.0) {
            vec3 delta = local - clamp(local, -half_extents, half_extents);
            distance = length(delta);
            local_normal = delta / distance;
        } else {
            // Inside: leave through the nearest face.
            distance = largest;
            if (outside.x == largest) local_normal = vec3(local.x < 0.0 ? -1.0 : 1.0, 0.0, 0.0);
            else if (outside.y == largest) local_normal = vec3(0.0, local.y < 0.0 ? -1.0 : 1.0, 0.0);
            else local_normal = vec3(0.0, 0.0, local.z < 0.0 ? -1.0 : 1.0);
        }
    } else {
        // A sphere is a capsule with no height.
        float half_height = shape.x < 1.5 ? 0.0 : shape.y;
        float radius = shape.x < 1.5 ? shape.y : shape.z;
        vec3 delta = local - vec3(0.0, clamp(local.y, -half_height, half_height), 0.0);
        float length_delta = length(delta);
        if (length_delta > 0.000001) local_normal = delta / length_delta;
        distance = length_delta - radius;
    }
    normal = rotation * local_normal;
}
