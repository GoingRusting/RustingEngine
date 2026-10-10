// Shared simulation math; see docs/determinism.md. Each routine uses only
// correctly rounded +, -, * and integer bit operations in a fixed order, so
// it returns the same bits on every device. `precise` keeps the compiler
// from fusing or reordering them; it only covers the function it is written
// in, so every intermediate here is a `precise` local.
// src/runtime/sim_math.rs is the bit-exact Rust reference; change both.
// ponytail: these run in every DeterminismMode, Off included; switch them to
// native built-ins through a specialization constant if profiling shows the
// cost.
#ifndef SIM_MATH_GLSL
#define SIM_MATH_GLSL

// Newton steps approach from below and stop an ULP short of round values
// such as 1 / 1. Each routine ends by keeping whichever of y - 1 ULP, y,
// and y + 1 ULP leaves the smallest residual.
float sim_neighbour(float y, int offset) {
    return uintBitsToFloat(uint(int(floatBitsToUint(y)) + offset));
}

// 1 / x for finite, normal, non-zero x, within 1 ULP.
float sim_recip(float x) {
    float magnitude = abs(x);
    // Bit-trick seed, then three Newton steps y = y * (2 - x * y).
    precise float y = uintBitsToFloat(0x7EF311C3u - floatBitsToUint(magnitude));
    for (int step = 0; step < 3; step++) {
        precise float error = 2.0 - magnitude * y;
        y = y * error;
    }
    float best = y;
    precise float best_residual = abs(1.0 - magnitude * y);
    for (int offset = -1; offset <= 1; offset += 2) {
        float candidate = sim_neighbour(y, offset);
        precise float residual = abs(1.0 - magnitude * candidate);
        if (residual < best_residual) {
            best = candidate;
            best_residual = residual;
        }
    }
    return x < 0.0 ? -best : best;
}

float sim_div(float a, float b) {
    precise float result = a * sim_recip(b);
    return result;
}

vec3 sim_div(vec3 v, float s) {
    precise vec3 result = v * sim_recip(s);
    return result;
}

// 1 / sqrt(x) for finite, normal, positive x, within 2 ULP.
float sim_rsqrt(float x) {
    precise float half_x = 0.5 * x;
    precise float y = uintBitsToFloat(0x5F375A86u - (floatBitsToUint(x) >> 1));
    for (int step = 0; step < 3; step++) {
        precise float square = half_x * y;
        square = square * y;
        precise float error = 1.5 - square;
        y = y * error;
    }
    float best = y;
    precise float best_residual = abs(1.0 - x * y * y);
    for (int offset = -1; offset <= 1; offset += 2) {
        float candidate = sim_neighbour(y, offset);
        precise float residual = abs(1.0 - x * candidate * candidate);
        if (residual < best_residual) {
            best = candidate;
            best_residual = residual;
        }
    }
    return best;
}

// sqrt(x); 0 for x <= 0.
float sim_sqrt(float x) {
    if (x <= 0.0) return 0.0;
    precise float root = x * sim_rsqrt(x);
    float best = root;
    precise float best_residual = abs(root * root - x);
    for (int offset = -1; offset <= 1; offset += 2) {
        float candidate = sim_neighbour(root, offset);
        precise float residual = abs(candidate * candidate - x);
        if (residual < best_residual) {
            best = candidate;
            best_residual = residual;
        }
    }
    return best;
}

float sim_dot(vec3 a, vec3 b) {
    precise float sum = a.x * b.x;
    sum = sum + a.y * b.y;
    sum = sum + a.z * b.z;
    return sum;
}

float sim_length(vec3 v) {
    return sim_sqrt(sim_dot(v, v));
}

// v scaled to length 1; the zero vector stays zero.
vec3 sim_normalize(vec3 v) {
    float square = sim_dot(v, v);
    if (square <= 0.0) return vec3(0.0);
    precise vec3 result = v * sim_rsqrt(square);
    return result;
}

// m * v with each row summed in x, y, z order.
vec3 sim_mul(mat3 m, vec3 v) {
    return vec3(
        sim_dot(vec3(m[0].x, m[1].x, m[2].x), v),
        sim_dot(vec3(m[0].y, m[1].y, m[2].y), v),
        sim_dot(vec3(m[0].z, m[1].z, m[2].z), v)
    );
}

// transpose(m) * v: each column dotted with v.
vec3 sim_mul_transposed(mat3 m, vec3 v) {
    return vec3(sim_dot(m[0], v), sim_dot(m[1], v), sim_dot(m[2], v));
}

// float to int the way Rust's `as i32` does it: truncate toward zero,
// saturate out-of-range values, NaN to 0. GLSL's int() leaves those cases
// undefined.
int sim_to_int(float x) {
    if (isnan(x)) return 0;
    if (x >= 2147483648.0) return 2147483647;
    if (x <= -2147483648.0) return int(0x80000000u);
    return int(x);
}

ivec3 sim_to_int(vec3 v) {
    return ivec3(sim_to_int(v.x), sim_to_int(v.y), sim_to_int(v.z));
}

// (sin x, cos x), as `sim_math::sin_cos`.
// ponytail: bit-equal to Rust only for |x| <= 8192; past that this reduces
// by 2 pi with floor where Rust uses `%`. Port an exact fmod if a sim ever
// feeds such angles to both sides.
vec2 sim_sin_cos(float x) {
    precise float magnitude = abs(x);
    if (magnitude > 8192.0) {
        precise float turns = floor(magnitude * 0.15915494);
        magnitude = magnitude - turns * 6.2831855;
    }
    precise float scaled = magnitude * 1.2732395;
    uint octant = uint(scaled);
    octant += octant & 1u;
    float octant_float = float(octant);
    precise float remainder = ((magnitude - octant_float * 0.78515625)
        - octant_float * 2.4187565e-4)
        - octant_float * 3.774895e-8;
    precise float square = remainder * remainder;
    precise float sine = ((-1.9515296e-4 * square + 8.332161e-3) * square
        - 1.6666655e-1)
        * square
        * remainder
        + remainder;
    precise float cosine = ((2.4433157e-5 * square - 1.3887316e-3) * square
        + 4.1666646e-2)
        * square
        * square
        - 0.5 * square
        + 1.0;
    uint turn = octant & 7u;
    vec2 result = turn == 0u ? vec2(sine, cosine)
        : turn == 2u ? vec2(cosine, -sine)
        : turn == 4u ? vec2(-sine, -cosine)
        : vec2(-cosine, sine);
    return vec2(x < 0.0 ? -result.x : result.x, result.y);
}

#endif
