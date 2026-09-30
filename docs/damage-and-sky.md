# Vehicle damage and seamless sky update (2026-09-19)

The original Bugatti mesh now has 20 breakable assemblies: doors, mirrors,
side/front/rear glass, four lights, front bumper, hood/luggage bodywork,
active rear wing, and four wheel assemblies. Impacts use the physics solver's
contact points and normal impulse, with distance falloff and accumulated damage.
Minor impacts dent vertices and scuff materials; strong impacts release the
original mesh as debris or shatter glass. Every car keeps its own material and
mesh copies so damage does not leak into another car.

Detached panels and wheels tumble and bounce against the environment. Debris
ignores other debris, vehicle bodies, and suspension rays, preventing self-hit
storms and random wheel kicks. Glass expires after 12 seconds, detached
assemblies stop simulating after 35 seconds. Resetting the car with R/new race
restores the original geometry, parenting, materials, lights and wheel support.

Offline, losing a wheel removes its suspension, traction and drive torque;
losing the wing removes its aerodynamic contribution. The roof, pillars,
fenders and main frame now form an additional non-detachable shell. High-energy
impacts compress a volume through the body; a frontal impact over 500 km/h can
collapse much of the shell instead of stopping at the previous 24 cm dent cap.
The deformation uses normal impact speed, direction and vehicle dimensions,
with bounded vertex displacement and deterministic folds. Damage is stored in
car coordinates so movement between physics/render frames cannot miss the mesh.
This is mesh deformation, not a soft-body collision solver.

Before each race the player chooses whether collisions can damage the car.
Collision response remains active in either setting.

Collision detection runs after every 120 Hz physics step rather than querying
only the last contact of a rendered frame. Material and geometry work runs
only when needed. This paragraph originally described the 2026-09-19 impact
bank; the active audio was replaced by the W16 Hypercar SFX pack on
2026-09-28. Its license is in `vendor/w16_audio/LICENSE-SOURCE.txt`.

Day and night skies now use 2048-pixel faces projected from continuous 8K
Poly Haven panoramas. Both cubemaps are lossless PNG, use the same projection
and exposure for all six faces, wrap longitude, and use linear cube sampling.
The 12 matching edge pairs are validated by tools/build_skybox.py. Their worst
mean adjacent RGB differences are 0.829/255 (day) and 1.964/255 (night), within
the natural image gradients. Original downloads and numeric reports remain in
.asset_sources/sky; credits are in assets/environment/skyboxes/license.txt.

Validation for the 2026-09-19 update: all 15 headless integration tests and
4 deformation tests passed, including a real 520 km/h collision.
Off-screen renders from the game are saved in docs/previews/sky-day-road.png
and sky-night-backward.png.
