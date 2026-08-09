use nalgebra::{Matrix4, Point3, Vector3};

// --- Camera (unchanged) ---

pub struct Frustum {
    pub(crate) planes: [[f32; 4]; 6],
}
impl Frustum {
    pub fn from_view_proj(vp: &Matrix4<f32>) -> Self {
        // nalgebra stores column-major; as_slice() returns in column-major order:
        //   col 0: m[0..4]  = elements (row 0, col 0), (row 1, col 0), (row 2, col 0), (row 3, col 0)
        //   col 1: m[4..8]
        //   col 2: m[8..12]
        //   col 3: m[12..16]
        //
        // clip_i = row_i · p = Σ P[i][j] * p[j]  (i = output component, j = input component)
        // In column-major storage, P[i][j] = m[i + j*4] (i = row, j = column).
        //
        // clip_x = row0 · p → uses m[0], m[4], m[8],  m[12]
        // clip_y = row1 · p → uses m[1], m[5], m[9],  m[13]
        // clip_z = row2 · p → uses m[2], m[6], m[10], m[14]
        // clip_w = row3 · p → uses m[3], m[7], m[11], m[15]
        //
        // Frustum planes (inside = dot(normal, point) + D >= 0):
        //   left:   x_ndc >= -1 → clip_x + clip_w >= 0  → (row0 + row3)·p >= 0
        //   right:  x_ndc <=  1 → clip_w - clip_x >= 0  → (row3 - row0)·p >= 0
        //   bottom: y_ndc >= -1 → clip_y + clip_w >= 0  → (row1 + row3)·p >= 0
        //   top:    y_ndc <=  1 → clip_w - clip_y >= 0  → (row3 - row1)·p >= 0
        // Vulkan's depth range is 0..1, unlike OpenGL's -1..1.
        //   near:   z_ndc >=  0 → clip_z >= 0           → row2·p >= 0
        //   far:    z_ndc <=  1 → clip_w - clip_z >= 0  → (row3 - row2)·p >= 0
        let m = vp.as_slice();
        Frustum {
            planes: [
                // left:   row 3 + row 0
                normalize_plane([m[3] + m[0], m[7] + m[4], m[11] + m[8], m[15] + m[12]]),
                // right:  row 3 - row 0
                normalize_plane([m[3] - m[0], m[7] - m[4], m[11] - m[8], m[15] - m[12]]),
                // bottom: row 3 + row 1
                normalize_plane([m[3] + m[1], m[7] + m[5], m[11] + m[9], m[15] + m[13]]),
                // top:    row 3 - row 1
                normalize_plane([m[3] - m[1], m[7] - m[5], m[11] - m[9], m[15] - m[13]]),
                // near: row 2
                normalize_plane([m[2], m[6], m[10], m[14]]),
                // far:    row 3 - row 2
                normalize_plane([m[3] - m[2], m[7] - m[6], m[11] - m[10], m[15] - m[14]]),
            ],
        }
    }
    pub fn test_aabb(&self, min: [f32; 3], max: [f32; 3]) -> bool {
        for p in &self.planes {
            let px = if p[0] >= 0.0 { max[0] } else { min[0] };
            let py = if p[1] >= 0.0 { max[1] } else { min[1] };
            let pz = if p[2] >= 0.0 { max[2] } else { min[2] };
            if p[0] * px + p[1] * py + p[2] * pz + p[3] < 0.0 {
                return false;
            }
        }
        true
    }
}
fn normalize_plane(p: [f32; 4]) -> [f32; 4] {
    let len = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
    if len < 1e-10 {
        return p;
    }
    [p[0] / len, p[1] / len, p[2] / len, p[3] / len]
}

#[derive(Clone)]
pub struct Camera {
    pub position: Point3<f32>,
    pub front: Vector3<f32>,
    pub right: Vector3<f32>,
    pub up: Vector3<f32>,
    pub yaw: f32,
    pub pitch: f32,
    pub fov: f32,
    pub aspect: f32,
    pub near: f32,
    pub far: f32,
    /// Third-person front view looks opposite the movement/look direction.
    pub reverse_view: bool,
    /// FOV multiplier (1.0 = normal, 60/70 ≈ 0.857 when underwater).
    pub fov_modifier: f32,
    /// Previous tick's fov_modifier, for smooth frame interpolation.
    pub prev_fov_modifier: f32,
    /// Frame interpolation fraction (0.0–1.0, set before each render).
    pub partial_tick: f32,
    /// First-person camera effects, updated by `Player` from vanilla-style limb
    /// animation and hurt timers.  Kept on the camera so every world pass uses
    /// the same transformed view matrix.
    pub view_bobbing: bool,
    pub bob_phase: f32,
    pub bob_amount: f32,
    pub bob_pitch: f32,
    pub hurt_time: f32,
    pub prev_hurt_time: f32,
    pub hurt_cam_enabled: bool,
    pub fov_change_enabled: bool,
}
impl Camera {
    pub fn new(position: Point3<f32>, aspect: f32) -> Self {
        let mut c = Camera {
            position,
            front: Vector3::new(0.0, 0.0, -1.0),
            right: Vector3::new(1.0, 0.0, 0.0),
            up: Vector3::new(0.0, 1.0, 0.0),
            yaw: -90.0f32.to_radians(),
            pitch: -30.0f32.to_radians(),
            fov: 80.0,
            aspect,
            // EntityRenderer.setupCameraTransform uses 0.05 in 1.8.9. A 0.1
            // near plane clips block faces while the eye is still outside the
            // collision volume, especially during interpolation and bobbing.
            near: 0.05,
            far: 500.0,
            reverse_view: false,
            fov_modifier: 1.0,
            prev_fov_modifier: 1.0,
            partial_tick: 0.0,
            view_bobbing: false,
            bob_phase: 0.0,
            bob_amount: 0.0,
            bob_pitch: 0.0,
            hurt_time: 0.0,
            prev_hurt_time: 0.0,
            hurt_cam_enabled: true,
            fov_change_enabled: true,
        };
        c.update_vectors();
        c
    }
    pub fn process_mouse(&mut self, dx: f32, dy: f32, sensitivity: f32, invert_y: bool) {
        // EntityRenderer.updateCameraAndRender in 1.8.9 uses
        // `(sensitivity * 0.6 + 0.2)^3 * 8`.  Preserve the previous 0.5
        // default turn rate and scale it by that vanilla curve.
        let vanilla = (sensitivity.clamp(0.0, 1.0) * 0.6 + 0.2).powi(3) * 8.0;
        let default_vanilla = (0.5_f32 * 0.6 + 0.2).powi(3) * 8.0;
        let scale = 0.003 * vanilla / default_vanilla;
        self.yaw += dx * scale;
        let pitch_delta = if invert_y { dy * scale } else { -dy * scale };
        self.pitch = (self.pitch + pitch_delta).clamp(-89.0f32.to_radians(), 89.0f32.to_radians());
        self.update_vectors();
    }
    fn update_vectors(&mut self) {
        self.front = Vector3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        )
        .normalize();
        self.right = self.front.cross(&Vector3::new(0.0, 1.0, 0.0)).normalize();
        self.up = self.right.cross(&self.front).normalize();
    }
    pub fn view_matrix(&self) -> Matrix4<f32> {
        let view_front = if self.reverse_view {
            -self.front
        } else {
            self.front
        };
        let mut view = Matrix4::look_at_rh(&self.position, &(self.position + view_front), &self.up);

        // EntityRenderer.setupViewBobbing — vanilla-compatible bobbing.
        // Transforms are camera-local (happen after the view transform),
        // matching vanilla's GL post-multiplication order conceptually.
        if self.view_bobbing && self.bob_amount > 0.0001 {
            let f1 = self.bob_phase;
            let f2 = self.bob_amount;
            let f3 = self.bob_pitch;

            let sin_f1 = (f1 * std::f32::consts::PI).sin();
            let cos_f1 = (f1 * std::f32::consts::PI).cos();

            let tx = sin_f1 * f2 * 0.5;
            let ty = -(cos_f1 * f2).abs();
            let roll = sin_f1 * f2 * 3.0_f32.to_radians();
            let pitch = ((f1 * std::f32::consts::PI - 0.2).cos().abs() * f2) * 5.0_f32.to_radians()
                + f3.to_radians();

            view = Matrix4::new_translation(&Vector3::new(tx, ty, 0.0))
                * Matrix4::from_euler_angles(pitch, 0.0, roll)
                * view;
        }

        // EntityRenderer.hurtCameraEffect — vanilla-compatible hurt tilt,
        // interpolated between ticks so the roll fades smoothly.
        if self.hurt_cam_enabled && self.hurt_time > 0.0 {
            let h =
                self.prev_hurt_time + (self.hurt_time - self.prev_hurt_time) * self.partial_tick;
            let f = (h / 0.45).clamp(0.0, 1.0);
            let roll = -(f * f * f * f * std::f32::consts::PI).sin() * 14.0_f32.to_radians();
            view = Matrix4::from_euler_angles(0.0, 0.0, roll) * view;
        }
        view
    }

    /// View matrix without walking-bob or hurt-camera effects.
    ///
    /// First-person arm and held-item meshes are built in view space and then
    /// converted to world space with the inverse of this matrix.  The rendering
    /// pipeline applies the full `view_matrix()` (including bob and hurt), so
    /// those effects must NOT be baked into the inverse used during mesh
    /// construction — otherwise the arm stays still while the world shakes.
    pub fn view_look_at_matrix(&self) -> Matrix4<f32> {
        let view_front = if self.reverse_view {
            -self.front
        } else {
            self.front
        };
        Matrix4::look_at_rh(&self.position, &(self.position + view_front), &self.up)
    }

    /// View transform for infinitely distant geometry such as the sky.
    ///
    /// Camera world translation is cancelled so the sky remains infinitely far
    /// away, while camera-local effects stay in the transform. Vanilla 1.8.9
    /// renders the sky after hurt-camera and view-bobbing transforms are applied.
    pub fn sky_view_matrix(&self) -> Matrix4<f32> {
        self.view_matrix() * Matrix4::new_translation(&self.position.coords)
    }
    /// Interpolated FOV modifier for smooth per-frame rendering.
    /// `alpha` is the fraction through the current tick (0.0–1.0).
    pub fn fov_modifier_at(&self, alpha: f32) -> f32 {
        self.prev_fov_modifier + (self.fov_modifier - self.prev_fov_modifier) * alpha
    }

    pub fn projection_matrix(&self) -> Matrix4<f32> {
        self.projection_matrix_at(1.0)
    }

    /// Projection matrix with interpolated FOV modifier.
    pub fn projection_matrix_at(&self, alpha: f32) -> Matrix4<f32> {
        let effective_fov = self.fov * self.fov_modifier_at(alpha);
        self.projection_matrix_for(effective_fov, self.near, self.far)
    }

    /// Vanilla's hand pass ignores sprint/bow FOV changes and uses a separate
    /// near/far range. Underwater is the one hand-FOV modifier it preserves.
    pub fn first_person_projection_matrix(&self, underwater: bool) -> Matrix4<f32> {
        self.projection_matrix_for(if underwater { 60.0 } else { 70.0 }, 0.05, self.far * 2.0)
    }

    fn projection_matrix_for(&self, fov_degrees: f32, near: f32, far: f32) -> Matrix4<f32> {
        let f = 1.0 / (fov_degrees.to_radians() / 2.0).tan();
        let n = near;
        let fa = far;
        // Right-handed Vulkan perspective, with Y negated for Vulkan clip space
        // (Y down). Vulkan clips depth in 0..w; using OpenGL's -w..w projection
        // made geometry near the camera disappear before the configured near plane.
        // Row-major form:
        //   [ f/a   0    0               0             ]
        //   [ 0    -f    0               0             ]
        //   [ 0     0     fa/(n-fa)       fa*n/(n-fa)  ]
        //   [ 0     0    -1              0             ]
        Matrix4::new(
            f / self.aspect,
            0.0,
            0.0,
            0.0,
            0.0,
            -f,
            0.0,
            0.0,
            0.0,
            0.0,
            fa / (n - fa),
            fa * n / (n - fa),
            0.0,
            0.0,
            -1.0,
            0.0,
        )
    }
    pub fn update_aspect(&mut self, w: u32, h: u32) {
        if h > 0 {
            self.aspect = w as f32 / h as f32;
        }
    }
    pub fn frustum(&self) -> Frustum {
        Frustum::from_view_proj(&(self.projection_matrix() * self.view_matrix()))
    }

    /// Convert internal camera yaw to MC 1.8.9 protocol yaw (degrees).
    ///
    /// Camera convention: yaw=0 → +X (east), increases CCW from above.
    /// MC protocol: yaw=0 → +Z (south), increases CW from above. Vanilla keeps
    /// this value continuous, including across full turns.
    /// camera_yaw = 90 + mc_yaw  →  mc_yaw = camera_yaw - 90
    pub fn mc_yaw_degrees(&self) -> f32 {
        self.yaw.to_degrees() - 90.0
    }

    /// Convert internal camera pitch to MC 1.8.9 protocol pitch (degrees).
    ///
    /// Camera convention: pitch>0 → looking UP.
    /// MC protocol: pitch>0 → looking DOWN.
    pub fn mc_pitch_degrees(&self) -> f32 {
        -self.pitch.to_degrees()
    }

    /// Set camera yaw from MC 1.8.9 protocol yaw (degrees).
    /// camera_yaw = 90 + mc_yaw
    pub fn set_from_mc_yaw(&mut self, mc_yaw: f32) {
        self.yaw = (mc_yaw + 90.0).to_radians();
        self.update_vectors();
    }

    /// Set camera pitch from MC 1.8.9 protocol pitch (degrees).
    /// camera_pitch = -mc_pitch
    pub fn set_from_mc_pitch(&mut self, mc_pitch: f32) {
        self.pitch = (-mc_pitch).to_radians();
        self.update_vectors();
    }

    /// Add a delta in MC protocol yaw degrees.
    /// camera_yaw_change = mc_yaw_change (since cam = 90 + mc, d(cam) = d(mc))
    pub fn add_mc_yaw(&mut self, delta_mc_yaw: f32) {
        self.yaw += delta_mc_yaw.to_radians();
        self.update_vectors();
    }

    /// Add a delta in MC protocol pitch degrees.
    pub fn add_mc_pitch(&mut self, delta_mc_pitch: f32) {
        self.pitch += (-delta_mc_pitch).to_radians();
        self.update_vectors();
    }
}
