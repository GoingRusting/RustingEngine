#version 450
layout(local_size_x = 256, local_size_y = 1, local_size_z = 1) in;

struct InstanceData {
    mat4 model;
    vec4 color;
    vec4 mat_props;
    vec4 velocity;
    vec4 angular_velocity;
    vec4 physic_props;
};

layout(set = 0, binding = 0) buffer ReadBuf  { InstanceData data[]; } read_buf;
layout(set = 0, binding = 1) buffer WriteBuf { InstanceData data[]; } write_buf;

layout(push_constant) uniform PushConstants {
    float dt;
    uint  total_objects;
    uint  offset;
    uint  count;
    uint  num_big_objects;
    uint  _pad[3];
    vec4  global_gravity;
} pc;

mat3 skew(vec3 v) {
    return mat3( 0.0, v.z,-v.y,
                -v.z, 0.0, v.x,
                 v.y,-v.x, 0.0);
}

void main() {
    uint i = gl_GlobalInvocationID.x + pc.offset;
    if (i >= pc.offset + pc.count) return;

    write_buf.data[i].model[3].y += 0.0;
    InstanceData me = read_buf.data[i];

    if (me.physic_props.y <= 0.0) {
        write_buf.data[i] = me;
        return;
    }

    vec3 pos = me.model[3].xyz;
    vec3 vel = me.velocity.xyz;
    vec3 ang_vel = me.angular_velocity.xyz;
    float type = me.physic_props.x;
    float mass = me.physic_props.y;

    vec3 scale = vec3(length(me.model[0].xyz), length(me.model[1].xyz), length(me.model[2].xyz));
    mat3 rotA = mat3(me.model[0].xyz / scale.x, me.model[1].xyz / scale.y, me.model[2].xyz / scale.z);

    if (length(ang_vel) > 0.001) {
        rotA += (skew(ang_vel) * rotA) * pc.dt;
        vec3 c0 = normalize(rotA[0]);
        vec3 c1 = normalize(rotA[1] - dot(c0, rotA[1]) * c0);
        rotA = mat3(c0, c1, cross(c0, c1));
    }

    me.model[0] = vec4(rotA[0] * scale.x, 0.0);
    me.model[1] = vec4(rotA[1] * scale.y, 0.0);
    me.model[2] = vec4(rotA[2] * scale.z, 0.0);

    float bound_me = (type > 0.5) ? scale.x : length(scale) * 0.5;

    for (uint j = 0; j < pc.total_objects; ++j) {
        if (i == j) continue;
        InstanceData other = read_buf.data[j];
        vec3 o_pos = other.model[3].xyz;
        vec3 d = o_pos - pos;
        vec3 o_scale = vec3(length(other.model[0].xyz), length(other.model[1].xyz), length(other.model[2].xyz));
        float bound_o = (other.physic_props.x > 0.5) ? o_scale.x : length(o_scale) * 0.5;
        float sum_r = bound_me + bound_o;
        if (dot(d, d) >= sum_r * sum_r) continue;

        float mass_o = (other.physic_props.y <= 0.0) ? 1.0e9 : other.physic_props.y;
        float ratio_me = mass_o / (mass + mass_o);

        float dist = length(d);
        float overlap = sum_r - dist;
        if (overlap <= 0.0) continue;
        vec3 normal = (dist > 1e-4) ? (-d / dist) : vec3(0.0, 1.0, 0.0);

        float pen = max(overlap - 0.005, 0.0);
        pos += normal * (pen * 0.2 * ratio_me);

        vec3 v_rel = vel - other.velocity.xyz;
        float vn = dot(v_rel, normal);
        if (vn < 0.0) {
            float e = (abs(vn) < 0.4) ? 0.0 : 0.3;
            float imp = -(1.0 + e) * vn / (1.0/mass + 1.0/mass_o);
            vel += (normal * imp) / mass;
        }
    }

    if (pos.y - bound_me < 0.0) {
        pos.y = bound_me;
        if (vel.y < -0.1) vel.y *= -0.3;
        else vel.y = 0.0;
    }

    vel += pc.global_gravity.xyz * me.physic_props.z * pc.dt;
    pos += vel * pc.dt;
    vel *= pow(0.993, pc.dt * 60.0);

    float spd = length(vel);
    if (spd < 0.08) vel *= (spd < 0.01) ? 0.0 : 0.85;

    me.model[3].xyz = pos;
    me.velocity.xyz = vel;
    me.angular_velocity.xyz = ang_vel;
    write_buf.data[i] = me;
}
