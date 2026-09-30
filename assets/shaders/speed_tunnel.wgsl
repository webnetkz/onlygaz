#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;

struct SpeedTunnelEffect {
    intensity: f32,
    radial_blur: f32,
    vignette: f32,
    chromatic_aberration: f32,
    nitro: f32,
    impact_blur: f32,
    impact_direction: vec2<f32>,
};

@group(0) @binding(2) var<uniform> effect: SpeedTunnelEffect;

fn read_screen(uv: vec2<f32>) -> vec4<f32> {
    return textureSample(screen_texture, screen_sampler, clamp(uv, vec2(0.0), vec2(1.0)));
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;

    // Both effects are uniform across the frame, so this branch is cheap and
    // a parked, unhurt driver receives an exact one-sample copy.
    if effect.intensity <= 0.0001 && effect.impact_blur <= 0.0001 {
        return read_screen(uv);
    }

    var output = read_screen(uv);

    if effect.intensity > 0.0001 {
        // Express the radius in viewport-relative coordinates. This gives the
        // clear region an oval/cinema-frame shape instead of a tiny circular
        // hole on wide screens.
        let from_focus = uv - vec2(0.5, 0.48);
        let radius = length(from_focus * 2.0);
        // Nitro compresses the clear focal island to a very small centre and
        // pulls maximum radial smearing farther inward.
        let clear_radius = mix(0.34, 0.08, effect.nitro);
        let full_blur_radius = mix(1.08, 0.68, effect.nitro);
        let peripheral = smoothstep(clear_radius, full_blur_radius, radius);

        // Symmetric samples along the ray from the vanishing point create a
        // genuine zoom/radial blur. Squaring the mask keeps the centre sharp.
        let streak = from_focus * effect.radial_blur * effect.intensity
            * peripheral * peripheral;
        var speed_color = read_screen(uv) * 0.22;
        speed_color += read_screen(uv - streak * 0.20) * 0.15;
        speed_color += read_screen(uv + streak * 0.20) * 0.15;
        speed_color += read_screen(uv - streak * 0.42) * 0.12;
        speed_color += read_screen(uv + streak * 0.42) * 0.12;
        speed_color += read_screen(uv - streak * 0.68) * 0.08;
        speed_color += read_screen(uv + streak * 0.68) * 0.08;
        speed_color += read_screen(uv - streak) * 0.04;
        speed_color += read_screen(uv + streak) * 0.04;

        // A restrained red/blue split makes the outer streaks read as speed.
        let direction = normalize(from_focus + vec2(0.000001, 0.0));
        let split = direction * effect.chromatic_aberration * effect.intensity
            * peripheral * peripheral;
        let split_mix = clamp(effect.intensity * peripheral * 0.72, 0.0, 1.0);
        let separated_red = mix(speed_color.r, read_screen(uv + split).r, split_mix);
        let separated_blue = mix(speed_color.b, read_screen(uv - split).b, split_mix);
        var processed = vec3(separated_red, speed_color.g, separated_blue);

        // Tunnel vision gently desaturates and darkens only the periphery.
        let luminance = dot(processed, vec3(0.2126, 0.7152, 0.0722));
        let desaturation = clamp(effect.intensity * peripheral * 0.16, 0.0, 1.0);
        processed = mix(processed, vec3(luminance), desaturation);
        let tunnel = smoothstep(0.48, 1.20, radius);
        let darkening = clamp(
            effect.vignette * effect.intensity * tunnel * tunnel,
            0.0,
            0.94,
        );
        processed *= 1.0 - darkening;
        output = vec4(processed, speed_color.a);
    }

    if effect.impact_blur > 0.0001 {
        let impact = clamp(effect.impact_blur, 0.0, 1.0);
        let primary = normalize(effect.impact_direction + vec2(0.000001, 0.0));
        let secondary = vec2(-primary.y, primary.x);
        let diagonal_a = normalize(primary + secondary);
        let diagonal_b = normalize(primary - secondary);
        // At full strength this spans roughly 60 pixels on a 1080p image.
        let radius = mix(0.006, 0.032, impact * impact);

        // A broad eleven-tap directional kernel destroys fine focus across the
        // whole image instead of preserving the speed effect's clear centre.
        var crash = read_screen(uv) * 0.12;
        crash += read_screen(uv + primary * radius) * 0.09;
        crash += read_screen(uv - primary * radius) * 0.09;
        crash += read_screen(uv + secondary * radius) * 0.09;
        crash += read_screen(uv - secondary * radius) * 0.09;
        crash += read_screen(uv + diagonal_a * radius * 0.78) * 0.07;
        crash += read_screen(uv - diagonal_a * radius * 0.78) * 0.07;
        crash += read_screen(uv + diagonal_b * radius * 0.78) * 0.07;
        crash += read_screen(uv - diagonal_b * radius * 0.78) * 0.07;
        crash += read_screen(uv + primary * radius * 1.65) * 0.12;
        crash += read_screen(uv - primary * radius * 1.65) * 0.12;

        // Offset colour channels create short-lived double vision, while
        // desaturation and dimming sell the stunned recovery without a flash.
        let ghost = primary * radius * 0.62;
        crash = vec4(
            mix(crash.r, read_screen(uv + ghost).r, impact * 0.55),
            crash.g,
            mix(crash.b, read_screen(uv - ghost).b, impact * 0.55),
            crash.a,
        );
        let crash_luminance = dot(crash.rgb, vec3(0.2126, 0.7152, 0.0722));
        let stunned = mix(crash.rgb, vec3(crash_luminance), impact * 0.42)
            * mix(1.0, 0.82, impact);
        crash = vec4(stunned, crash.a);

        output = vec4(
            mix(output.rgb, crash.rgb, clamp(impact * 1.15, 0.0, 1.0)),
            output.a,
        );
    }

    return output;
}
