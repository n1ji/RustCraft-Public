//! Backend-independent Minecraft sky and celestial calculations.

/// GPU-facing sky/light values derived from Minecraft time.
#[derive(Clone, Copy, Debug)]
pub struct SkyEnvironment {
    pub clear_color: [f32; 4],
    pub fog_color: [f32; 4],
    pub light_dir: [f32; 4],
    pub fog_params: [f32; 4],
}

/// Sky gradient colors for different times of day.
pub struct SkyGradient;

impl SkyGradient {
    const DAY_TICKS: f32 = 24000.0;

    pub fn environment(day_time: i64, render_distance: u8, dimension: i8) -> SkyEnvironment {
        match dimension {
            -1 => Self::fixed_environment(
                [0.18, 0.035, 0.025, 1.0],
                [0.33, 0.055, 0.035, 1.0],
                [0.35, -0.65, 0.25],
                0.48,
                0.35,
                render_distance,
            ),
            1 => Self::fixed_environment(
                [0.012, 0.010, 0.030, 1.0],
                [0.035, 0.025, 0.060, 1.0],
                [0.20, -0.35, 0.85],
                0.30,
                0.10,
                render_distance,
            ),
            _ => Self::overworld_environment(day_time, render_distance),
        }
    }

    fn overworld_environment(day_time: i64, render_distance: u8) -> SkyEnvironment {
        let ticks = day_time as f32;
        let day = Self::daylight_factor(ticks);
        let dusk = Self::sunset_factor(ticks);
        let ambient = lerp(0.18, 0.46, day) + dusk * 0.04;
        let fog = Self::fog_color(ticks);
        let clear = Self::clear_color(ticks);
        let far = (render_distance.max(2) as f32 * 16.0).max(48.0);

        SkyEnvironment {
            clear_color: clear,
            fog_color: fog,
            light_dir: with_w(Self::sun_direction(ticks), ambient),
            fog_params: [far * 0.72, far, ambient, day],
        }
    }

    fn fixed_environment(
        clear_color: [f32; 4],
        fog_color: [f32; 4],
        light_dir: [f32; 3],
        ambient: f32,
        daylight: f32,
        render_distance: u8,
    ) -> SkyEnvironment {
        let far = (render_distance.max(2) as f32 * 16.0).max(48.0);
        SkyEnvironment {
            clear_color,
            fog_color,
            light_dir: with_w(normalize(light_dir), ambient),
            fog_params: [far * 0.45, far * 0.82, ambient, daylight],
        }
    }

    /// MC 1.8.9 matching celestial angle (used by sky rendering).
    pub fn celestial_angle(time_of_day: f32) -> f32 {
        let t = (time_of_day % Self::DAY_TICKS) / Self::DAY_TICKS;
        let mut f = t - 0.25;
        if f < 0.0 {
            f += 1.0;
        }
        if f > 1.0 {
            f -= 1.0;
        }
        let original = f;
        let eased = 1.0 - ((f * std::f32::consts::PI).cos() + 1.0) * 0.5;
        original + (eased - original) / 3.0
    }

    pub fn daylight_factor(time_of_day: f32) -> f32 {
        (Self::celestial_angle(time_of_day)
            .mul_add(std::f32::consts::TAU, 0.0)
            .cos()
            * 2.0
            + 0.5)
            .clamp(0.0, 1.0)
    }

    pub fn sun_brightness(time_of_day: f32, rain: f32, thunder: f32) -> f32 {
        let angle = Self::celestial_angle(time_of_day) * std::f32::consts::TAU;
        let mut brightness = 1.0 - (1.0 - (angle.cos() * 2.0 + 0.2)).clamp(0.0, 1.0);
        brightness *= 1.0 - rain.clamp(0.0, 1.0) * 5.0 / 16.0;
        brightness *= 1.0 - thunder.clamp(0.0, 1.0) * 5.0 / 16.0;
        brightness
    }

    pub fn sunset_factor(time_of_day: f32) -> f32 {
        let t = (time_of_day % Self::DAY_TICKS) / Self::DAY_TICKS;
        let angle = (t - 0.25) * std::f32::consts::TAU;
        let horizon = 1.0 - angle.cos().abs();
        let daylight = Self::daylight_factor(time_of_day);
        (horizon.clamp(0.0, 1.0).powi(5) * (1.0 - (daylight - 0.5).abs() * 1.4)).clamp(0.0, 1.0)
    }

    pub fn clear_color(time_of_day: f32) -> [f32; 4] {
        let top = Self::zenith_color(time_of_day);
        let horizon = Self::horizon_color(time_of_day);
        [
            lerp(horizon[0], top[0], 0.42),
            lerp(horizon[1], top[1], 0.42),
            lerp(horizon[2], top[2], 0.42),
            1.0,
        ]
    }

    pub fn zenith_color(time_of_day: f32) -> [f32; 3] {
        let day = Self::daylight_factor(time_of_day);
        [0.29 * day, 0.53 * day, 0.88 * day]
    }

    pub fn horizon_color(time_of_day: f32) -> [f32; 3] {
        let day = Self::daylight_factor(time_of_day);
        let brightness = day * 0.94 + 0.06;
        [0.69 * brightness, 0.77 * brightness, 0.89 * day + 0.11]
    }

    pub fn fog_color(time_of_day: f32) -> [f32; 4] {
        let h = Self::horizon_color(time_of_day);
        [h[0], h[1], h[2], 1.0]
    }

    pub fn sun_color(time_of_day: f32) -> [f32; 4] {
        let direction = Self::sun_direction(time_of_day);
        let elevation = -direction[1];
        if elevation < -0.1 {
            [0.0, 0.0, 0.0, 0.0]
        } else {
            let brightness = elevation.clamp(0.0, 1.0);
            let sf = (1.0 - elevation).powi(2).min(1.0);
            [1.0 - sf * 0.2, 1.0 - sf * 0.5, 1.0 - sf * 0.7, brightness]
        }
    }

    pub fn sun_direction(time_of_day: f32) -> [f32; 3] {
        let angle = Self::celestial_angle(time_of_day) * std::f32::consts::TAU;
        [angle.sin(), -angle.cos(), 0.0]
    }

    pub fn moon_phase(day_time: i64) -> u8 {
        ((day_time / 24000).rem_euclid(8)) as u8
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len <= f32::EPSILON {
        [0.0, -1.0, 0.0]
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}

fn with_w(v: [f32; 3], w: f32) -> [f32; 4] {
    [v[0], v[1], v[2], w]
}

#[cfg(test)]
mod tests {
    use super::SkyGradient;

    #[test]
    fn daylight_matches_vanilla_cycle() {
        assert!((SkyGradient::daylight_factor(6000.0) - 1.0).abs() < 0.0001);
        assert!(SkyGradient::daylight_factor(18000.0) < 0.0001);
    }

    #[test]
    fn moon_phase_wraps_over_eight_days() {
        assert_eq!(SkyGradient::moon_phase(0), 0);
        assert_eq!(SkyGradient::moon_phase(7 * 24000), 7);
        assert_eq!(SkyGradient::moon_phase(8 * 24000), 0);
    }

    #[test]
    fn sunrise_glow_is_limited_to_the_horizon() {
        assert!(SkyGradient::sunset_factor(0.0) > 0.0);
        assert_eq!(SkyGradient::sunset_factor(6000.0), 0.0);
        assert_eq!(SkyGradient::sunset_factor(18000.0), 0.0);
    }
}
