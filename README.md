# Veyron Sim

A driving simulator prototype in Rust with eleven selectable cars — from the
**Bugatti Veyron 16.4 Super Sport** and **Mercedes-AMG ONE** to the
**Koenigsegg Jesko**, **Ferrari Enzo**, four Lamborghinis and two classic Fords
— on a straight 33-kilometre, six-lane road, built on **Bevy 0.19** and
**Avian 3D 0.7**.

It is not an arcade racer. The car is a single rigid body and everything that
makes it behave like a car - engine torque curve, seven-speed gearbox, all-wheel
drive, four independent ray-cast suspension units, a Pacejka-style tyre model
with finite grip, aerodynamic drag and downforce - is expressed as forces applied
to that body. Nothing anywhere sets a velocity or writes a transform.

---

## Requirements

| | |
|---|---|
| Rust toolchain | stable **1.95** or newer (edition 2024) |
| Engine | [Bevy](https://bevy.org) **0.19** |
| Physics | [Avian 3D](https://github.com/avianphysics/avian) **0.7** |
| Model format | glTF 2.0 (`.gltf` + `.bin` + textures, or `.glb`) |

Linux additionally needs the usual Bevy development packages
(`libasound2-dev`, `libudev-dev`, `libwayland-dev`, `libxkbcommon-dev`,
`libx11-dev`). macOS and Windows need nothing extra.

## Running

```bash
cargo run --release
```

The first build takes a few minutes because Bevy is compiled from source. Debug
builds are usable too - the `dev` profile compiles all dependencies with
`opt-level = 3` so the simulation still runs at full speed while your own code
compiles quickly:

```bash
cargo run
```

После нажатия `PLAY` выберите заезд с разрушениями или без них. Игра работает
локально; на трассе остаются машины NPC с регулируемой плотностью трафика.

Игра запускается в полноэкранном режиме 1920×1080. Нажмите `Esc` во время
заезда и откройте **Настройки графики**, чтобы в любой момент переключить
режим «на весь экран / в окне», выбрать разрешение 1280×720–3840×2160 и
настроить изображение по отдельности.

Переключатель **«Время суток»** находится прямо в основном меню паузы и
мгновенно меняет день на ночь и обратно. Ночью фары автоматически включаются
и освещают дорогу с тройной интенсивностью.

Кнопка **«В главное меню»** возвращает из заезда к выбору автомобиля без выхода
из игры. Во время заезда курсор мыши скрывается и захватывается окном; в паузе
и на экране выбора автомобиля он автоматически освобождается и снова
показывается.

В паузе доступен раздел **Управление**. Нажмите на основную или дополнительную
клавишу рядом с действием, затем нажмите новую клавишу на клавиатуре. Основная
клавиша заменяется, а дополнительная позволяет оставить два разных варианта
для одного действия; уже занятые клавиши отклоняются.

В разделе **Настройки звука** находятся независимые уровни громкости эффектов,
двигателя и музыки. Во время езды играет радио из 13 треков: порядок каждый раз
перемешивается, после последнего трека начинается новый случайный цикл без
повтора на границе. `9` переключает на предыдущий трек, `0` — на следующий.
В разделе **Управление** обе клавиши можно переназначить; в **Настройках звука**
есть кнопки переключения и название текущего трека. При паузе радио
приостанавливается, а в главном меню играет отдельная музыка.

Все изменения экрана, графики, звука и назначений клавиш сохраняются сразу и
автоматически восстанавливаются при следующем запуске. На macOS файл находится
в `~/Library/Application Support/Veyron Sim/settings.ron`, а в автономной
Windows-сборке — в `%APPDATA%\Veyron Sim\settings.ron`.

The complete in-game interface uses the bundled Ubuntu Regular/Bold typeface
and a unified English-only dark racing theme. Graphics settings include
independent **Tyre marks** and **Tyre smoke** selectors. The Performance preset
starts with tyre marks, smoke, live mirrors, speed/nitro FX, collision blur and other
optional post-processing off; switching to it also removes already-visible
smoke and rubber immediately. Every individual selector can then be changed,
including atmospheric haze, and the menu marks the result as a custom
configuration. These choices are saved in `settings.ron`.

В **Настройках графики** также доступна плотность живого трафика: «Выкл»,
«Низкая», «Средняя» и «Высокая». Поток использует городской автобус и машины
из гаража, движется в обоих направлениях по всем полосам, держит безопасную
дистанцию и физически взаимодействует с автомобилем игрока. Скорость любой
машины трафика жёстко ограничена 160 км/ч. Выбранная плотность сохраняется
вместе с остальными настройками.

## Controls

| key | action |
|---|---|
| `W` | throttle |
| `S` | brake; hold at a standstill to select `R`, then keep holding to reverse |
| `A` / `D` | steer left / right |
| `Space` | handbrake; also lights the brake lamps |
| `Left Shift` / `Right Shift` | extreme nitro boost, speed blur and blue exhaust flames |
| `C` | next camera |
| `R` | reset the car onto the nearest lane |
| `B` | hold for rear view; releasing restores the current forward camera (interior views use the roof) |
| `<` / `>` | hold for the car's left / right profile; releasing restores the selected camera |
| `F8` | toggle ABS |
| `F3` | telemetry overlay |
| `F4` | physics visualisation |
| `F5` | next lighting tier |
| `F7` | triple engine power and remove the top-speed rev limit (`POWER x3`) |
| `M` | mute the engine |
| `Esc` | pause menu |
| `Q` | quit (from the pause menu) |

На экране выбора: `A` / `D` или `←` / `→` листают машины, `Enter` или `Space`
подтверждает выбор.

## Vehicles and model licensing

The Ford Mustang GT3 preset and source model remain available for internal
physics/model tests, but the car is deliberately excluded from the player-facing
selection list. Its scene is still aligned non-destructively: only an explicit
list of wheel meshes is re-parented for animation. Every other original scene
node, including the driver/pylon and the complete aero package, remains attached
to the body.

The Mustang model is third-party content with a separate non-commercial
Creative Commons licence. Attribution and redistribution terms are recorded in
[`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md) and alongside the asset in
`assets/models/mustang_gt3/license.txt`.

SLS AMG использует 571-сильный M159, 7-ступенчатую DCT и развесовку 47:53.
Zonda Cinque получает
678-сильный AMG V12, 6-ступенчатый секвентал и собственную аэродинамику. Для
каждой машины отдельно заданы масса, инерция, центр тяжести, шины, подвеска,
тормоза, рулевое управление, коллайдеры, камеры, свет и звуковой профиль.

В расширенный гараж также входят Mercedes-AMG ONE, Lamborghini Aventador,
Koenigsegg Jesko, Ferrari Enzo, Lamborghini Countach 5000 QV, Lamborghini
Revuelto, Ford GT 2006 и Ford Mustang 1965. Их исходные масштабы и оси
нормализованы, объединённые колёсные меши безопасно разделены там, где это
позволяет структура модели, а конфигурации двигателя, коробки, привода,
аэродинамики, шин, тормозов, света, салонной камеры и четырёхслойного звука
задаются отдельно для каждого автомобиля.

Throttle, brake and steering are all smoothed: a key press ramps the pedal over
a couple of hundred milliseconds instead of slamming it, and the steering rack
has separate rates towards lock and back to centre. The raw device layer
(`DriverInput`) is separate from the smoothing layer, so adding a gamepad means
writing one more system into `DriverInput` and changing nothing else.

To back up, hold `S` while stopped until the HUD shows `R`, then keep holding
the same key. In reverse, the brake pedal is intentionally repurposed as the
reverse accelerator; pressing `W` at any reverse speed immediately selects
first gear, slows the backward motion and starts accelerating forwards.

Above 200 km/h the steering input rate is reduced progressively, with a smooth
transition rather than a step at the threshold. At 400 km/h steering builds at
35 % of its normal rate; return to centre stays faster for stability. The
existing speed-dependent road-wheel angle curve remains in place as well.

До 45 км/ч включается отдельная аркадная помощь рулению: клавиатурный руль
быстрее набирает угол, максимальный выворот колёс увеличивается, а ограниченный
по сцеплению момент помогает кузову сразу начать поворот. Помощь плавно
уменьшается и полностью отключается на 100 км/ч, поэтому средне- и
высокоскоростная управляемость остаётся прежней.

## Cameras

`C` cycles through exactly six modes:

1. **Chase Close** - behind the car, whole car in frame.
2. **Chase Far** - higher, further back, wider.
3. **Front Bumper** - 35 cm off the tarmac at the nose.
4. **Hood** - above the bonnet, bonnet visible along the bottom edge.
5. **Cockpit** - centred between the seats, dashboard and steering wheel in view.
6. **Steering Wheel** - the individual driver's eye point directly behind the
   steering wheel of the selected car.

The cockpit eye point uses a fixed local Z offset of +0.35 m. Its mount is no
longer exposed in the graphics menu.

The two chase cameras use a yaw-only frame so the horizon does not roll. Their
orbit offsets are smoothed while the car's already interpolated translation is
applied directly. This removes the fixed-physics-tick impulses that otherwise
show up as camera shake, and it prevents speed-dependent trailing in turns.

The four on-board cameras - bumper, bonnet, cockpit and steering wheel - are rigidly bolted to
the car. Their complete position and rotation are rebuilt from the car's local
frame every rendered frame, with no smoothing or independent motion. Their
field of view is fixed as well, so speed cannot make a mounted camera appear to
slide or zoom.

At speeds above **120 km/h**, every camera and quality tier gains a progressively
stronger speed effect. The centre of the frame stays sharp while the periphery
develops radial motion blur, chromatic separation and a darkened, slightly
desaturated tunnel-vision mask. At 400 km/h the effect is 2.5 times stronger
than the original maximum and eases in and out over time, so crossing the
threshold never causes a visible pop. It is applied to the 3D image before the
UI pass, keeping the speedometer and telemetry perfectly readable.

A collision now drives a separate full-frame loss-of-focus effect from the
solved contact impulse. Small accepted impacts recover in one second; severe
crashes produce stronger directional blur, brief double vision, desaturation
and a smooth recovery lasting up to five seconds. The HUD remains sharp, and
**Эффект столкновения** can be disabled independently in graphics settings.

The car's rigid body carries `TransformInterpolation`, so its rendered transform
is interpolated between physics ticks. Without it the car only moves on physics
ticks, and at 120 Hz physics against a 60 or 144 Hz screen every rigidly
attached camera inherits that stutter.

The chase-camera field of view follows a configurable curve - 65 degrees at
rest rising to 80 degrees at 400 km/h. Mounted views stay at 65 degrees. All of
it lives in `src/camera/modes.rs`.

---

## Lighting

Three tiers, picked at start-up with the `VEYRON_QUALITY` environment variable
and cycled at run time with `F5`:

```bash
VEYRON_QUALITY=cinematic cargo run --release
VEYRON_QUALITY=balanced  cargo run --release
VEYRON_QUALITY=performance cargo run --release # the default
```

| tier | what runs |
|---|---|
| `performance` | authored skybox, one directional sun, flat ambient, distance fog, 2x MSAA and two hard-shadow cascades. No cinematic lighting passes; this is the default. |
| `balanced` | authored skybox, PCSS soft shadows, contact shadows, high SSAO, bloom, ACES tonemapping, TAA plus contrast adaptive sharpening. |
| `cinematic` | as balanced, plus screen space reflections through the deferred path, ultra SSAO, a 4096² shadow map and a touch of motion blur. |

Только пресет `cinematic` добавляет вдоль всей трассы плотный еловый лес из
набора Realistic Fir Trees Pack. Лес подгружается 300-метровыми участками вокруг
машины: ближние ряды используют LOD0/LOD1, средние — LOD2, дальние — billboard
LOD3. В `performance` и `balanced` модели и текстуры деревьев не запрашиваются,
поэтому в минимальном режиме окружение остаётся простой плоскостью.

Внутри меню доступны отдельные уровни для теней, эффекта ускорения, SSAO,
отражений и атмосферы, а также VSync, MSAA/TAA, дальность прорисовки,
контактные тени, bloom, motion blur, эффект столкновения, резкость и экспозиция. Изменения
применяются сразу; выбор пресета заполняет все эти параметры согласованным
профилем, после чего их можно изменить вручную.

The two upper tiers add the closest practical equivalents to modern cinematic
rendering without ray tracing hardware:

* **Two authored skies.** Day uses a clean generated sky, while night uses the
  24K-source Qwantani Moon Noon (Pure Sky) HDRI. The night panorama is converted
  to 2048-pixel cubemap faces. Switching time of day in the pause menu changes
  the sky, sun, fog, exposure and automatic headlights together.
* **Short-range occlusion is put back.** A directional light cannot express the
  darkening inside a wheel arch or the contact between tyre and tarmac, so SSAO
  and screen-space contact shadows do it instead.
* **Shadows have a penumbra.** Percentage-closer soft shadows
  (`experimental_pbr_pcss`) widen the shadow edge with the distance between
  occluder and receiver, temporally filtered and resolved by TAA.
* **The road reflects.** Asphalt is authored just inside the screen space
  reflection roughness cut-off, so the tarmac carries a soft reflection of the
  sky and of the car instead of being a flat grey plane.
* **Exposure is physical.** HDR throughout, ev100 15 ("sunny 16"), ACES
  tonemapping and a restrained bloom.

### Real ray tracing

Bevy ships `bevy_solari`, a ray-traced direct-and-indirect lighting system that
is a true Lumen equivalent. It needs `EXPERIMENTAL_RAY_QUERY`, which wgpu
exposes on Vulkan and DirectX 12 but **not on Metal**, so it cannot run on a
Mac and is not wired in here. On a Windows or Linux box with a ray tracing GPU
it is a small addition: add `bevy_solari`, add `SolariPlugins`, put
`SolariLighting` on the camera and mirror every `Mesh3d` with a
`RaytracingMesh3d`.

### Cost

`performance` is the default because it keeps the driving loop responsive on a
wide range of GPUs. `cinematic` asks for a deferred pass, three screen-space
passes and a ray-marched sky; select it explicitly only when the GPU has enough
headroom. Flat world surfaces are excluded from the shadow pass because they
cannot cast a useful shadow onto the scene.

Press `Esc` to open the pause menu and choose `PERFORMANCE`, `BALANCED` or
`CINEMATIC` with the mouse. The complete renderer changes live without a
restart. Cinematic retains the original maximum-quality image; Performance is
the 60-120 FPS target. `F5` remains available as a quick preset shortcut.

For a repeatable local measurement, set `VEYRON_BENCHMARK_SECONDS` to a positive
number. After a three-second warm-up the game measures that interval, prints the
average and 1% low FPS, then exits.

---

## Sound

The player car now uses the supplied W16 Hypercar SFX pack. Its 48 kHz Vorbis
loops crossfade between recorded RPM points and between on-throttle and
off-throttle states; cockpit views use the separate interior set. Turbo,
blow-off, shift accents, rolling tyres, wind, braking, wheelspin, impacts,
glass and menu feedback also come from this pack. The mixer is fed by the
physics state, not key presses. Silent engine layers are paused, and only the
player car gets the full multi-layer bank. The engine and effects sliders
remain independent; `M` mutes the engine without muting the tyres or impacts.
The pack's licence is recorded in `vendor/w16_audio/LICENSE-SOURCE.txt`.

The main, vehicle-selection and pause menus loop **Raim - Where Are You**.
Playback pauses during the race and uses the saved music-volume setting.
During a race, the in-car radio plays the 13 bundled MP3 tracks in a shuffled
order. It pauses with the race and uses the same music-volume slider.

---

## Screenshots

`docs/screenshots/` holds stills of four of the six camera modes, rendered on
the cinematic lighting tier. They are produced by the project itself rather
than by a screen grab, which means they can be regenerated on a build machine
with no display and used as a visual regression check:

```bash
cargo run --release --example render_still -- shot.png 3.0 0 0.0
#                                             file    settle camera throttle
```

The four positional arguments are the output path, how many seconds to let the
world settle and the assets load, which camera mode to select (`0` chase close,
`1` chase far, `2` front bumper, `3` bonnet, `4` cockpit), and how many seconds
to hold the throttle before the shot, so the car is not parked. Resolution
defaults to 1280x720 and is overridden with `VEYRON_SHOT_WIDTH` and
`VEYRON_SHOT_HEIGHT`.

The example never opens a window: it renders into an off-screen image and reads
it back. On a machine with no GPU a software Vulkan driver is enough
(`mesa-vulkan-drivers` on Debian and Ubuntu), though a frame then takes seconds
rather than milliseconds. Two details in the harness matter and are worth
knowing before you change it: the camera renders into a 64x64 target while the
world settles so assets and GPU pipelines can finish loading cheaply; after the
switch to the full-size target it runs a further 28 frames before capturing,
because temporal anti-aliasing and the temporal shadow filter accumulate across
frames. Capture too early and you get a noisy picture that says nothing about
the scene.

The renderer also prints the camera's position in the car's own frame, which is
how the on-board camera placements in `camera/modes.rs` were checked against the
real geometry of the model.

---

## Why Avian and not Rapier

Both would work; Avian fits this particular job better.

* **The rigid-body state is plain ECS data.** `Position`, `Rotation`,
  `LinearVelocity`, `AngularVelocity`, `ComputedCenterOfMass` are components a
  normal Bevy system reads and writes. A custom vehicle model is mostly "read the
  body state, compute forces, apply forces", and with Avian that is one query -
  there is no separate physics world to marshal data in and out of.
* **`Forces` is exactly the API this needs.** Avian 0.7 exposes a `Forces` query
  data type with `apply_force_at_point` and `velocity_at_point`, which is
  precisely the vocabulary of a suspension-and-tyre model.
* **The schedule is open.** The vehicle system is registered directly into
  `PhysicsSchedule` at `PhysicsStepSystems::First`, so the forces it accumulates
  are consumed by the very next solver iteration rather than a frame later.
* **No built-in vehicle controller to fight.** Rapier ships a raycast vehicle
  controller, but it is a game-style controller with a simplified tyre model.
  This project wants its own tyre model, so that head start is not worth much -
  and working *around* an engine's vehicle abstraction is harder than working
  with a plain rigid body.
* Pure Rust, no C++ bindings, and an `enhanced-determinism` feature if
  reproducibility ever needs to be tightened further.

Rapier would be the better pick for a project that wanted a ready-made vehicle
controller, or that needed Rapier's more mature joint set.

---

## Project structure

```
src/
  main.rs                  Bevy app assembly, nothing else
  game.rs                  plugin wiring, app state, global keys
  lib.rs

  vehicle/
    mod.rs                 VehiclePlugin, spawning, placeholder visuals
    config.rs              VehicleConfig and every sub-configuration (pure data)
    presets.rs             the Veyron itself
    engine.rs              torque curve, idle control, rev limiter
    transmission.rs        7-speed gearbox and the automatic shift strategy
    drivetrain.rs          AWD split, viscous differentials, reflected inertia
    brakes.rs              friction brakes, handbrake, ABS
    tire.rs                Magic Formula tyre with combined slip and relaxation
    aero.rs                drag and downforce
    wheels.rs              suspension springs, dampers, anti-roll bars, ray probe
    solver.rs              the vehicle model - pure, engine-agnostic
    testbed.rs             a 6-DOF integrator so the solver can be tested headless
    physics.rs             Avian glue: rigid body, forces, visual rig update
    controller.rs          input collection and smoothing
    model_rig.rs           glTF discovery and wheel re-pivoting
    lights.rs              brake lamps: emissive rear clusters and their spill
    telemetry.rs           live 0-100 / braking / top speed measurement

  audio/
    mod.rs                 EngineAudioPlugin
    engine.rs              two-voice engine mixer driven by the physics

  camera/
    mod.rs                 CameraPlugin
    modes.rs               the six modes and the FOV curve
    controller.rs          smoothing, lag, follow logic

  world/
    mod.rs                 WorldPlugin - builds the whole track
    road.rs                RoadSpec, surface, markings, kilometre boards
    barriers.rs            guard rails and their colliders
    lighting.rs            quality tiers, day/night skyboxes, sun, post-processing
    mesh_builder.rs        procedural mesh accumulator + seven-segment digits

  ui/
    mod.rs                 UiPlugin
    speedometer.rs         speed / gear / rpm / camera HUD and tachometer bar
    debug.rs               F3 telemetry overlay
    gizmos.rs              F4 physics visualisation
    menu.rs                pause overlay

tests/
  physics_validation.rs    acceleration, braking, top speed, stability
  app_smoke.rs             the real Bevy app, built and stepped without a window

examples/
  render_still.rs          renders one frame to a PNG with no window

assets/
  models/bugatti/          the glTF car
  ui/vehicle_logos/        official manufacturer marks displayed from SVG
  environment/skyboxes/    clean day and high-resolution night cubemaps
  audio/w16/               W16 game sounds, OGG only
  radio/                   existing in-car radio tracks
  music/                   looping menu soundtrack

docs/
  model_analysis.md        what is actually inside the supplied glTF
  screenshots/             stills rendered head-lessly by examples/render_still
```

The split that matters is between the **pure model** (`config`, `presets`,
`engine`, `transmission`, `drivetrain`, `brakes`, `tire`, `aero`, `wheels`,
`solver`, `testbed`) which contains no Bevy systems and no physics-engine types,
and the **integration** (`physics`, `controller`, `model_rig`, `telemetry`).
That is what makes `cargo test` able to measure a 0-300 km/h run in a fraction
of a second without opening a window.

---

## The physics model

### Chain of causality

```
throttle -> engine torque curve -> clutch -> gearbox -> centre differential
         -> axle differentials -> wheel angular acceleration -> tyre slip
         -> tyre force at the contact patch -> chassis acceleration
```

Each arrow is a real computation. There is no `velocity = input * max_speed`
anywhere, and the top speed is not clamped - it is where thrust and drag happen
to balance.

### Suspension

Each corner casts a ray from its strut anchor along the car's own down axis. The
distance to the road gives the suspension length, from which the model derives
compression, compression *rate*, and hence

```
F = k * x  +  k_progressive * x^3  +  c_bump|c_rebound * v  +  bump stops  +  ARB
```

with separate bump and rebound damping and an anti-roll bar term proportional to
the compression difference across the axle. The resulting normal force is applied
along the contact normal at the contact point, so braking dives, accelerating
squats and cornering rolls all fall out of the geometry rather than being faked.

### Tyres

A Pacejka-style Magic Formula is evaluated for the longitudinal and lateral
directions and then combined through a friction ellipse expressed in normalised
slip:

```
s  = sqrt( (kappa/kappa_peak)^2 + (tan(alpha)/alpha_peak)^2 )
Fx = (|kappa/kappa_peak| / s) * Fx_pure(kappa)
Fy = (|tan(alpha)/alpha_peak| / s) * Fy_pure(alpha)
```

Pure slip reproduces the pure curves exactly, and equal slip in both directions
lands precisely on the ellipse. Grip is finite and load-sensitive
(`mu = mu_peak * (1 - sensitivity * (Fz/Fz_nom - 1))`), which is what produces
load transfer sensitivity, wheel spin, lock-up and slides.

Slip is not read straight off the kinematics: it passes through a **relaxation
length** filter, the same first-order lag a real carcass has. That keeps the
stiff tyre ODE stable near zero speed without any of the usual fudge factors.

### Wheels

Every wheel is integrated as its own rotating body:

```
I_eff * dw/dt = T_drive - T_brake - Fx * r - T_rolling_resistance
```

`I_eff` includes the engine's inertia reflected through the current gear
(`I_engine * ratio^2`), which is why the car cannot spin its tyres instantly in
first. The `- Fx * r` term is the tyre reacting back on the hub - that single
term is what makes wheel spin and lock-up emerge instead of being scripted.

The wheel/tyre ODE is sub-stepped (`VehicleConfig::solver_substeps`, default 4)
inside each 120 Hz physics tick, so the effective tyre rate is 480 Hz.

### Aerodynamics

`F = 0.5 * rho * C * A * v^2` for drag, and separately for front and rear
downforce, each applied at its own point so the pitch moment is right. At
415 km/h the car sees roughly 6.4 kN of drag and 4.8 kN of downforce, and that
downforce shows up directly as extra vertical load on the tyres.

### Steering

Maximum road-wheel angle follows a speed curve (35 degrees at rest, 1.8 degrees
at 400 km/h), and the two front wheels are given proper **Ackermann** angles so
the inside wheel turns further than the outside one.

---

## Car configuration

Everything is in `VehicleConfig`, and `presets::bugatti_veyron_super_sport()` is
the only place the Veyron's numbers appear:

| parameter | value |
|---|---|
| mass | 1888 kg |
| inertia (pitch / yaw / roll) | 2700 / 2900 / 700 kg·m² |
| centre of mass | (0, 0.44, +0.199) m - 45/55 front/rear, 0.44 m up |
| wheelbase / tracks | 2.707 m / 1.727 m front, 1.615 m rear |
| engine | W16 quad-turbo, 1500 N·m from 3000-5000 rpm, ~1200 PS at 6400 rpm |
| idle / redline / limiter | 900 / 6800 / 6900 rpm |
| gearbox | 7 speeds: 2.83, 1.95, 1.46, 1.14, 0.92, 0.73, 0.60; final drive 3.44 |
| drivetrain | AWD, 30 % front / 70 % rear, 45 % differential locking |
| tyres | 0.340 m front, 0.358 m rear; peak mu 1.45 / 1.50 |
| suspension | 76 / 88 kN/m, ~2.1 Hz, 100 mm bump, 90 mm droop |
| brakes | 5200 N·m front, 3200 N·m rear per wheel, ABS at 0.14 slip |
| aero | Cd 0.38, A 2.07 m², Cl 0.114 front + 0.170 rear |

The tyre radii are the *loaded rolling* radii, which is why they are 5 mm
smaller than the unloaded geometric radii measured off the model - a loaded tyre
really is squashed by about that much.

To add another car, write another function that returns a `VehicleConfig`. The
solver has no idea which car it is running.

## Car models

The car model lives in `assets/models/bugatti/scene.gltf`. If it is missing the
game starts anyway with a primitive placeholder body and cylinder wheels, which
is enough to develop and test the physics against - but it is a fallback, not the
intended result.

`docs/model_analysis.md` documents what the supplied file contains. At start-up
`model_rig.rs` repeats that analysis on the live scene and prints it, for
example:

```
vehicle model: 501 nodes scanned, 24 node groups re-pivoted
  FL visual hub (0.864, 0.341, 1.290) vs physical anchor (0.864, 0.620, 1.291)
  ...
  measured wheelbase 2.704 m (config 2.707 m), front track 1.727 m (config 1.727 m)
  steering wheel pivot at (0.388, 0.766, 0.540)
```

### Adapting a different model

Node names are matched in `model_rig::classify`. To use another car:

1. Run the game once and read the log. Anything not recognised stays on the body.
2. Add the new name fragments to `ROTOR_PATTERNS`, `CALIPER_PATTERNS`,
   `TYRE_PATTERNS` or `RIM_PREFIX`. A node's whole subtree follows it, so only
   the top of each group needs to match.
3. If a corner matches nothing at all, the rig falls back to claiming unassigned
   meshes whose centroid is within 62 cm of that corner's hub, and warns.
4. Adjust `VehicleConfig::wheels[..].anchor` if the log reports the visual hubs
   are far from the physical anchors.

Two intermediate entities are created per corner: a **hub pivot** (suspension
travel + steering, brake calipers hang here) and a **spin pivot** (wheel
rotation, rim/tyre/disc hang here). Pivot positions are measured from the meshes,
so a model whose wheel pivots are at the origin - as this one's are - still
rotates correctly.

### Brake lamps

`lights.rs` does the same kind of discovery for the rear light clusters, and
prints what it found:

```
  brake lamp left centred at (0.478, 0.790, -2.127)
  brake lamp right centred at (-0.478, 0.790, -2.127)
brake lamps: 5 surfaces lit, 2 spill lights
```

Five surfaces light up and nothing else: the red lens of each rear cluster, the
lit element behind it, and the high mounted centre strip under the rear screen.
Each of them gets its own **clone** of the material it was authored with, because
the clusters share `lights_chrome` and `detail_glass_red` with the headlights,
the mirror shells and the red side markers on the wings, and writing emissive
into a shared asset would light all of those up too. A red point light at the
measured centre of each cluster throws a little spill onto the diffuser.

The lamps behave like a switch rather than a dimmer: they come on within the
first 2 % of pedal travel and reach full output in about 40 ms, however hard the
car is being stopped, and the handbrake lights them as well - not what a real
handbrake does, but the only way a driver watching from behind can tell the car
is being pulled up on the lever. Brightness, spill and ramp rates are the
constants at the top of `lights.rs`; setting a luminance to zero drops that
surface out of the effect.

The lamps are emissive surfaces, so their halo is the camera's bloom: it is
there on `balanced` and `cinematic`, and absent on `performance`, which runs no
post-processing by design and simply lights the lamps up without glowing. Their
luminances are given in real cd/m2 and metered through the camera's exposure
like every other surface - sunlit asphalt here is about 2 000 cd/m2 and a sunlit
lane marking about 18 000 - so they hold their look when the tier or
`VEYRON_EXPOSURE` moves the exposure by several stops.

---

## The track

* A straight **33 000 m** road running along world **+Z**, connected into an
  endless loop: crossing either endpoint preserves the car's complete dynamic
  state and continues at the opposite end.
* Six lanes of 3.6 m: three in each direction, plus 1.6 m paved shoulders.
* A **double solid** centre line, dashed lane separators (3 m dash, 9 m gap) and
  solid edge lines.
* Steel guard rails down both sides with physical box colliders. Seamless,
  infinitely tall backstop planes immediately behind them prevent even an
  airborne high-speed impact from throwing the car outside the track.
* Kilometre boards from 1 to 33, drawn with seven-segment digits so world
  markers do not depend on the UI font. The pause and graphics menus bundle
  Ubuntu Regular/Bold with full Cyrillic coverage in `assets/fonts/ubuntu/`.
* Grass, roadside posts and low-polygon hills on the horizon.

Visual geometry is split into 100 m chunks (one merged mesh for the
terrain, one for the road surface, one for every marking in the chunk, and one
per side for the guard rail) so frustum culling has something to work with. The
road uses one continuous collider so high-speed CCD cannot catch internal
segment faces. A 4 km visual and physical continuation beyond both endpoints
hides the loop transition from the main camera and rear-view mirrors. Asphalt
and this flat collider extend one metre beyond the axis
of each guard rail, preventing an outside wheel from dropping into a terrain
step. Guard rails retain per-chunk contact colliders, while the two safety
backstops and surrounding terrain use seamless half-spaces.

Traffic keeps right and the car spawns in the middle lane of the right-hand
carriageway. NPC positions and following distances use the same cyclic road
coordinates, including across the start/finish seam. Respawn preserves the
current distance and snaps the car back to the nearest lane. A car that remains
on its roof for 1.5 seconds is recovered through the same clean physics reset as
`R`. Locked tyres and driven-wheel spin emit expanding contact-patch smoke;
both emergency braking and power wheelspin lay individual, correctly sized tyre
tracks using each car's real front/rear tyre widths.

---

## HUD and debugging

The HUD shows speed, gear, engine speed, camera mode, a tachometer bar that goes
red past the redline, and status flags (`SHIFT`, `ABS`, `SLIP`, `HANDBRAKE`,
`LIMITER`).

`F3` adds the full telemetry overlay: frame rate, position, pedal positions,
clutch state, engine and axle torque, power, drag, downforce, longitudinal and
lateral g, and a per-wheel table of wheel rpm, surface speed, slip ratio, slip
angle, vertical load, longitudinal and lateral force, suspension travel and brake
torque - plus the running 0-100 / 0-200 / 0-300 / braking / top-speed records.

`F4` draws the physics: body collider boxes, centre of mass, suspension rays,
wheel circles, contact points, and the normal, traction and lateral force vectors
at every contact patch.

## Verification

```bash
cargo fmt
cargo clippy --all-targets
cargo test
cargo test --test physics_validation -- --nocapture   # the full numbers table
```

`tests/physics_validation.rs` drives the real solver through a headless
integrator and asserts the figures the brief calls for: 0-100, 0-200 and
0-300 km/h times, 100-0 and 200-0 braking distances with and without ABS, top
speed, per-gear rpm and speed, straight-line stability, behaviour under a full
lock swerve at 250 km/h, and bit-for-bit determinism of two identical runs.

## Known limitations

* **The tyre model has no temperature, wear or camber.** Grip depends on vertical
  load and slip only. A thermal model is the obvious next step for a full
  simulator.
* **No aerodynamic yaw sensitivity.** Drag and downforce depend on speed, not on
  slip angle, so there is no aero snap in a big slide.
* **The differentials are viscous couplings**, not modelled clutch-pack LSDs, and
  there is no torque vectoring.
* **The gearbox is automatic only.** `ShiftMode::Manual` exists and the solver
  honours it, but no key is bound to paddles yet.
* **No tyre-to-body collision.** Wheels are ray casts; only the body compound
  collider can hit the guard rails.
* **Steering feel is open loop.** There is no self-aligning torque fed back to
  the input, which a force-feedback wheel would want.
* **The road is perfectly flat and perfectly grippy.** No camber, crown, bumps,
  kerbs or surface variation - the suspension is fully capable of them, the track
  generator simply does not produce any yet.
* **Cockpit view depends on the model's interior.** The supplied Veyron has a
  full cockpit, but the dashboard needles are static geometry - only the steering
  wheel is animated.
* **The brake lamps are the only lights that work.** No side lights, headlights,
  indicators or reversing lights, and no night to need them: the sun is fixed at
  midday.
* **No volumetric fog or light shafts.** Bevy has them, but they need bounded
  `FogVolume`s, and a volume big enough for a 33 km road is not worth the
  raymarching cost; distance fog covers the same ground at much lower cost.
* **Vehicle and road geometry have no authored LODs.** The track is chunked and
  frustum culled, while the Cinematic forest uses all four supplied tree LODs
  across progressively deeper rows.

## Licence and credits

Project code: MIT OR Apache-2.0.

The car models are **not** covered by that licence. The original Veyron is
["2009 Bugatti Veyron 16.4"](https://sketchfab.com/3d-models/2009-bugatti-veyron-164-33da16dacdb34d8088324a24645a1802)
by **Ddiaz Design**, licensed **CC-BY-NC-SA 4.0**: attribution required, no
commercial use, and derivatives must carry the same licence.

Mustang, Mercedes-Benz and Pagani model attribution and their exact
Creative Commons terms are recorded in
[`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md); every original archive
licence is also preserved beside its model.

The Cinematic-only fir forest uses **“Realistic Fir Trees Pack (LODS,
gameready)”** by LOLIPOP under CC BY 4.0. Full attribution and the preserved
licence text are in [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md) and
`assets/environment/fir_trees/license.txt`.

The daytime skybox was generated specifically for the game and contains only
sky and clouds, with no photographed ground or scenery. The high-resolution
night skybox is adapted from **Qwantani Moon Noon (Pure Sky)** by Greg Zaal and
Jarod Guest, released under CC0. Source links and conversion details are in
[`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md) and
`assets/environment/skyboxes/license.txt`.
