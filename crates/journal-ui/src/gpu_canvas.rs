//! WebGPU / WebGL2 ambient kozo paper fiber & warm desk-lamp shader canvas.
//!
//! Degrades silently with zero panics to an SVG `feTurbulence` paper-grain
//! backdrop when WebGPU and WebGL2 contexts are unavailable (e.g. headless QA),
//! and provides a clean host stub when compiled on non-WASM targets for workspace checks.

use leptos::prelude::*;

#[cfg(target_arch = "wasm32")]
mod wasm_impl {
    use std::cell::RefCell;
    use std::rc::Rc;

    use leptos::prelude::*;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::spawn_local;
    use wgpu::util::DeviceExt;

    const PAPER_SHADER_WGSL: &str = r#"
struct Uniforms {
    resolution: vec2<f32>,
    cursor: vec2<f32>,
    time: f32,
    night_mix: f32,
    _pad0: f32,
    _pad1: f32,
};

@group(0) @binding(0)
var<uniform> u: Uniforms;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VsOut {
    var out: VsOut;
    let x = f32(i32(idx) / 2) * 4.0 - 1.0;
    let y = f32(i32(idx) % 2) * 4.0 - 1.0;
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, 1.0 - (y + 1.0) * 0.5);
    return out;
}

fn hash21(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(123.34, 456.21));
    q += dot(q, q + 45.32);
    return fract(q.x * q.y);
}

fn noise2(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u_f = f * f * (3.0 - 2.0 * f);
    let a = hash21(i);
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u_f.x), mix(c, d, u_f.x), u_f.y);
}

fn fbm(p: vec2<f32>) -> f32 {
    var v = 0.0;
    var amp = 0.5;
    var pos = p;
    for (var i = 0; i < 4; i = i + 1) {
        v += amp * noise2(pos);
        pos = pos * 2.03 + vec2<f32>(17.1, 31.7);
        amp *= 0.5;
    }
    return v;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let aspect = max(u.resolution.x / max(u.resolution.y, 1.0), 0.5);
    let p_aspect = vec2<f32>(in.uv.x * aspect, in.uv.y);
    let c_aspect = vec2<f32>(u.cursor.x * aspect, u.cursor.y);

    // Multi-octave kozo paper fiber grain
    let macro_wash = fbm(p_aspect * 3.2);
    let fine_grain = noise2(in.uv * u.resolution * 0.42);
    let kozo_h = fbm(vec2<f32>(p_aspect.x * 18.0, p_aspect.y * 115.0));
    let kozo_d = fbm(vec2<f32>((p_aspect.x + p_aspect.y) * 58.0, (p_aspect.x - p_aspect.y) * 22.0));
    let fiber = (kozo_h * 0.55 + kozo_d * 0.45) - 0.5;

    // Gentle desk vignette
    let centered = in.uv - vec2<f32>(0.5, 0.5);
    let vignette = smoothstep(0.92, 0.16, length(centered));

    // Cursor lamp falloff
    let dist = distance(p_aspect, c_aspect);
    let lamp_breath = 1.0 + 0.018 * sin(u.time * 0.65);
    let day_sun = exp(-dist * dist * 2.2) * 0.045;
    let night_lamp = exp(-dist * dist * 2.8) * 0.24 * lamp_breath;

    // Day Cream Paper (#EDE5D7 desk -> #FBF7F0 warm light)
    let day_base = vec3<f32>(0.935, 0.905, 0.852);
    let day_warm = vec3<f32>(0.984, 0.969, 0.941);
    var day_col = mix(day_base, day_warm, 0.35 + 0.45 * vignette + 0.2 * macro_wash);
    day_col += vec3<f32>(0.045, 0.038, 0.025) * day_sun;
    day_col += vec3<f32>(0.022, 0.019, 0.015) * fiber;
    day_col += vec3<f32>(0.010, 0.009, 0.008) * (fine_grain - 0.5);

    // Candlelit Night Desk (#141212 slate + #D99B52 warm amber desk lamp)
    let night_base = vec3<f32>(0.075, 0.068, 0.068);
    let night_elev = vec3<f32>(0.112, 0.098, 0.094);
    let amber_glow = vec3<f32>(0.851, 0.608, 0.322);
    var night_col = mix(night_base, night_elev, 0.35 + 0.55 * vignette + 0.15 * macro_wash);
    night_col = mix(night_col, amber_glow, clamp(night_lamp, 0.0, 0.22));
    night_col += vec3<f32>(0.018, 0.014, 0.010) * fiber * (0.6 + night_lamp * 2.0);
    night_col += vec3<f32>(0.007, 0.006, 0.006) * (fine_grain - 0.5);

    let color = mix(day_col, night_col, clamp(u.night_mix, 0.0, 1.0));
    return vec4<f32>(color, 1.0);
}
"#;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct PaperUniforms {
        resolution: [f32; 2],
        cursor: [f32; 2],
        time: f32,
        night_mix: f32,
        _pad0: f32,
        _pad1: f32,
    }

    impl PaperUniforms {
        fn as_bytes(&self) -> [u8; 32] {
            let mut bytes = [0u8; 32];
            let vals = [
                self.resolution[0],
                self.resolution[1],
                self.cursor[0],
                self.cursor[1],
                self.time,
                self.night_mix,
                self._pad0,
                self._pad1,
            ];
            for (idx, val) in vals.iter().enumerate() {
                let chunk = val.to_le_bytes();
                bytes[idx * 4..idx * 4 + 4].copy_from_slice(&chunk);
            }
            bytes
        }
    }

    pub struct SharedCanvasState {
        pub target_cursor: [f32; 2],
        pub smooth_cursor: [f32; 2],
        pub target_night: f32,
        pub smooth_night: f32,
    }

    pub fn mount_gpu_paper(
        canvas_ref: NodeRef<leptos::html::Canvas>,
        night_mode: Signal<bool>,
        gpu_active: RwSignal<bool>,
    ) {
        let shared_state = Rc::new(RefCell::new(SharedCanvasState {
            target_cursor: [0.52, 0.34],
            smooth_cursor: [0.52, 0.34],
            target_night: if night_mode.get_untracked() { 1.0 } else { 0.0 },
            smooth_night: if night_mode.get_untracked() { 1.0 } else { 0.0 },
        }));

        {
            let shared = shared_state.clone();
            Effect::new(move |_| {
                let night = night_mode.get();
                shared.borrow_mut().target_night = if night { 1.0 } else { 0.0 };
            });
        }

        {
            let shared = shared_state.clone();
            Effect::new(move |_| {
                let Some(window) = web_sys::window() else {
                    return;
                };
                let shared_move = shared.clone();
                let closure = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(
                    move |ev: web_sys::MouseEvent| {
                        let Some(win) = web_sys::window() else {
                            return;
                        };
                        let w = win
                            .inner_width()
                            .ok()
                            .and_then(|v| v.as_f64())
                            .unwrap_or(1280.0)
                            .max(1.0);
                        let h = win
                            .inner_height()
                            .ok()
                            .and_then(|v| v.as_f64())
                            .unwrap_or(800.0)
                            .max(1.0);
                        let nx = (ev.client_x() as f64 / w).clamp(0.0, 1.0) as f32;
                        let ny = (ev.client_y() as f64 / h).clamp(0.0, 1.0) as f32;
                        shared_move.borrow_mut().target_cursor = [nx, ny];
                    },
                );
                let _ = window.add_event_listener_with_callback(
                    "mousemove",
                    closure.as_ref().unchecked_ref(),
                );
                closure.forget();
            });
        }

        {
            let shared = shared_state;
            let started = Rc::new(RefCell::new(false));
            Effect::new(move |_| {
                let Some(canvas) = canvas_ref.get() else {
                    return;
                };
                if *started.borrow() {
                    return;
                }
                *started.borrow_mut() = true;

                let shared_run = shared.clone();
                spawn_local(async move {
                    let _ = run_wgpu_paper_loop(canvas, shared_run, gpu_active).await;
                });
            });
        }
    }

    fn desired_canvas_size() -> (u32, u32) {
        let Some(window) = web_sys::window() else {
            return (1280, 720);
        };
        let dpr = window.device_pixel_ratio().clamp(1.0, 1.5);
        let w = window
            .inner_width()
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(1280.0)
            .max(320.0);
        let h = window
            .inner_height()
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(720.0)
            .max(240.0);
        let cw = ((w * dpr).round() as u32).clamp(320, 2048);
        let ch = ((h * dpr).round() as u32).clamp(240, 2048);
        (cw, ch)
    }

    async fn run_wgpu_paper_loop(
        canvas: web_sys::HtmlCanvasElement,
        shared: Rc<RefCell<SharedCanvasState>>,
        gpu_active: RwSignal<bool>,
    ) -> Option<()> {
        let (init_w, init_h) = desired_canvas_size();
        canvas.set_width(init_w);
        canvas.set_height(init_h);

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .ok()?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await?;

        let limits =
            wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits());
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("log-paper-device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: limits,
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await
            .ok()?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .or_else(|| caps.formats.first().copied())?;
        let alpha_mode = caps.alpha_modes.first().copied()?;

        let mut config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: init_w,
            height: init_h,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("log-paper-shader"),
            source: wgpu::ShaderSource::Wgsl(PAPER_SHADER_WGSL.into()),
        });

        let initial_uniforms = PaperUniforms {
            resolution: [init_w as f32, init_h as f32],
            cursor: [0.52, 0.34],
            time: 0.0,
            night_mix: shared.borrow().smooth_night,
            _pad0: 0.0,
            _pad1: 0.0,
        };

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("log-paper-uniforms"),
            contents: &initial_uniforms.as_bytes(),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("log-paper-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("log-paper-bg"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("log-paper-pipeline-layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("log-paper-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let start_ms = web_sys::window()
            .and_then(|w| w.performance())
            .map(|p| p.now())
            .unwrap_or(0.0);

        let raf_handle: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
        let raf_loop = raf_handle.clone();
        let mut marked_active = false;

        *raf_loop.borrow_mut() = Some(Closure::new(move || {
            let (target_w, target_h) = desired_canvas_size();
            if target_w != config.width || target_h != config.height {
                canvas.set_width(target_w);
                canvas.set_height(target_h);
                config.width = target_w;
                config.height = target_h;
                surface.configure(&device, &config);
            }

            let now_ms = web_sys::window()
                .and_then(|w| w.performance())
                .map(|p| p.now())
                .unwrap_or(start_ms);
            let elapsed = ((now_ms - start_ms) * 0.001) as f32;

            let (cx, cy, night_mix) = {
                let mut st = shared.borrow_mut();
                st.smooth_cursor[0] += (st.target_cursor[0] - st.smooth_cursor[0]) * 0.08;
                st.smooth_cursor[1] += (st.target_cursor[1] - st.smooth_cursor[1]) * 0.08;
                st.smooth_night += (st.target_night - st.smooth_night) * 0.08;
                (st.smooth_cursor[0], st.smooth_cursor[1], st.smooth_night)
            };

            let uniforms = PaperUniforms {
                resolution: [config.width as f32, config.height as f32],
                cursor: [cx, cy],
                time: elapsed,
                night_mix,
                _pad0: 0.0,
                _pad1: 0.0,
            };
            queue.write_buffer(&uniform_buffer, 0, &uniforms.as_bytes());

            let frame = match surface.get_current_texture() {
                Ok(f) => f,
                Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                    surface.configure(&device, &config);
                    schedule_next(&raf_handle);
                    return;
                }
                Err(wgpu::SurfaceError::Timeout) => {
                    schedule_next(&raf_handle);
                    return;
                }
                Err(wgpu::SurfaceError::OutOfMemory | wgpu::SurfaceError::Other) => {
                    gpu_active.set(false);
                    return;
                }
            };

            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("log-paper-encoder"),
            });

            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("log-paper-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    occlusion_query_set: None,
                    timestamp_writes: None,
                });
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.draw(0..3, 0..1);
            }

            queue.submit(Some(encoder.finish()));
            frame.present();

            if !marked_active {
                marked_active = true;
                gpu_active.set(true);
            }

            schedule_next(&raf_handle);
        }));

        schedule_next(&raf_loop);
        Some(())
    }

    fn schedule_next(raf: &Rc<RefCell<Option<Closure<dyn FnMut()>>>>) {
        if let Some(window) = web_sys::window() {
            if let Some(cb) = raf.borrow().as_ref() {
                let _ = window.request_animation_frame(cb.as_ref().unchecked_ref());
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod host_stub {
    use leptos::prelude::*;

    pub fn mount_gpu_paper(
        _canvas_ref: NodeRef<leptos::html::Canvas>,
        _night_mode: Signal<bool>,
        _gpu_active: RwSignal<bool>,
    ) {
        // Non-WASM host checks render the SVG feTurbulence paper fallback.
    }
}

/// Optional imperative initializer kept for backwards-compatibility with the workspace skeleton.
pub fn init_ambient_canvas(_canvas_id: &str) {
    // Lifecycle is managed directly by `GpuPaperCanvas`.
}

/// Ambient full-bleed `wgpu 24` kozo paper fiber & warm lamp glow canvas.
///
/// Silently degrades to an SVG `feTurbulence` paper-grain backdrop if WebGPU/WebGL2
/// cannot be initialized.
#[component]
pub fn GpuPaperCanvas(night_mode: Signal<bool>) -> impl IntoView {
    let canvas_ref = NodeRef::<leptos::html::Canvas>::new();
    let gpu_active = RwSignal::new(false);

    #[cfg(target_arch = "wasm32")]
    wasm_impl::mount_gpu_paper(canvas_ref, night_mode, gpu_active);

    #[cfg(not(target_arch = "wasm32"))]
    host_stub::mount_gpu_paper(canvas_ref, night_mode, gpu_active);

    view! {
        <div class="gpu-paper-backdrop" aria-hidden="true">
            <div class=move || {
                if gpu_active.get() {
                    "paper-fallback-layer is-gpu-ready"
                } else {
                    "paper-fallback-layer"
                }
            }>
                <div class="paper-fallback-grain"></div>
                <div class="paper-fallback-fibers"></div>
            </div>
            <canvas
                node_ref=canvas_ref
                id="log-paper-canvas"
                class=move || {
                    if gpu_active.get() {
                        "gpu-paper-canvas is-active"
                    } else {
                        "gpu-paper-canvas"
                    }
                }
            ></canvas>
        </div>
    }
}
