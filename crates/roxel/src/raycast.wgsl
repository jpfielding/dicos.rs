// Volume ray-casting shader.
//
// Renders a 3D volume by marching rays through a bounding box and
// sampling a 3D texture. Uses front-to-back alpha blending with
// gradient-based Phong lighting and a 1D transfer function lookup.

// Per-frame uniforms.
struct Uniforms {
    cam_pos: vec3<f32>,
    fov: f32,
    cam_forward: vec3<f32>,
    aspect_ratio: f32,
    cam_right: vec3<f32>,
    step_size: f32,
    cam_up: vec3<f32>,
    scale_z: f32,
    window_min: f32,
    window_range: f32,
    alpha_scale: f32,
    rescale_intercept: f32,
    density_threshold: f32,
    ambient_intensity: f32,
    diffuse_intensity: f32,
    specular_intensity: f32,
}

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var volume_tex: texture_3d<f32>;
@group(0) @binding(2) var volume_sampler: sampler;
@group(0) @binding(3) var transfer_tex: texture_2d<f32>;
@group(0) @binding(4) var transfer_sampler: sampler;

// Vertex shader: full-screen triangle (3 vertices, no buffers needed).
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOutput {
    // Full-screen triangle: vertices at (-1,-1), (3,-1), (-1,3).
    var out: VertexOutput;
    let x = f32(i32(idx & 1u) * 4 - 1);
    let y = f32(i32(idx >> 1u) * 4 - 1);
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

// Ray-box intersection for a box from (0,0,0) to (1,1,scale_z).
fn intersect_box(origin: vec3<f32>, dir: vec3<f32>) -> vec2<f32> {
    let box_max = vec3<f32>(1.0, 1.0, u.scale_z);
    let inv_dir = 1.0 / dir;

    let t0 = (vec3<f32>(0.0) - origin) * inv_dir;
    let t1 = (box_max - origin) * inv_dir;

    let tmin = min(t0, t1);
    let tmax = max(t0, t1);

    let t_near = max(max(tmin.x, tmin.y), tmin.z);
    let t_far  = min(min(tmax.x, tmax.y), tmax.z);

    return vec2<f32>(max(t_near, 0.0), t_far);
}

// Fragment shader: ray-cast through the volume.
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Generate ray direction from UV coordinates.
    let uv = in.uv * 2.0 - 1.0;
    let ray_dir = normalize(
        u.cam_forward
        + u.cam_right * uv.x * u.fov * u.aspect_ratio
        + u.cam_up * uv.y * u.fov
    );

    // Intersect with volume bounding box.
    let t_range = intersect_box(u.cam_pos, ray_dir);
    if t_range.x >= t_range.y {
        return vec4<f32>(0.84, 0.84, 0.84, 1.0); // Background color.
    }

    // Light directions.
    let light_main = normalize(vec3<f32>(0.5, 0.7, 1.0));
    let light_fill = normalize(vec3<f32>(-0.3, 0.2, 0.8));

    // March through volume.
    var accum = vec4<f32>(0.0);
    var t = t_range.x;
    let max_steps = 4096u;

    for (var step = 0u; step < max_steps; step++) {
        if t >= t_range.y || accum.a > 0.98 {
            break;
        }

        let pos = u.cam_pos + ray_dir * t;

        // Normalize to [0,1] texture coordinates.
        let tex_coord = vec3<f32>(pos.x, pos.y, pos.z / u.scale_z);

        // Sample volume.
        let sample = textureSampleLevel(volume_tex, volume_sampler, tex_coord, 0.0);
        let raw_density = sample.r;

        // Window/level normalization.
        let density = raw_density * 65535.0;
        let normalized = clamp(
            (density + u.rescale_intercept - u.window_min) / u.window_range,
            0.0, 1.0
        );

        // Density threshold clipping.
        if normalized <= u.density_threshold {
            t += u.step_size;
            continue;
        }

        // Transfer function lookup.
        let tf_sample = textureSampleLevel(transfer_tex, transfer_sampler, vec2<f32>(normalized, 0.5), 0.0);
        var color = tf_sample.rgb;
        var alpha = tf_sample.a * u.alpha_scale;

        if alpha < 0.001 {
            t += u.step_size;
            continue;
        }

        // Gradient from pre-computed channels (offset by 0.5).
        let gradient = vec3<f32>(
            sample.g - 0.5,
            sample.b - 0.5,
            sample.a - 0.5,
        );

        let grad_mag = length(gradient);
        if grad_mag > 0.01 {
            let normal = gradient / grad_mag;

            // Phong lighting.
            let n_dot_l1 = max(dot(normal, light_main), 0.0);
            let n_dot_l2 = max(dot(normal, light_fill), 0.0);

            let diffuse = u.diffuse_intensity * (n_dot_l1 * 0.7 + n_dot_l2 * 0.3);
            let ambient = u.ambient_intensity;

            // Specular (Blinn-Phong).
            let view_dir = normalize(u.cam_pos - pos);
            let half_dir = normalize(light_main + view_dir);
            let spec = pow(max(dot(normal, half_dir), 0.0), 32.0);

            color = color * (ambient + diffuse) + vec3<f32>(spec * u.specular_intensity);

            // Edge enhancement.
            alpha *= 1.0 + grad_mag * 0.5;
        } else {
            color = color * u.ambient_intensity;
        }

        // Front-to-back blending.
        let src_alpha = alpha * (1.0 - accum.a);
        accum = vec4<f32>(
            accum.rgb + color * src_alpha,
            accum.a + src_alpha,
        );

        t += u.step_size;
    }

    // Blend with background.
    let bg = vec3<f32>(0.84, 0.84, 0.84);
    let final_color = accum.rgb + bg * (1.0 - accum.a);
    return vec4<f32>(final_color, 1.0);
}
