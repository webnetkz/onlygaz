# glTF model analysis

The car model shipped in `assets/models/bugatti/` is a Sketchfab export of
*"2009 Bugatti Veyron 16.4"* by **Ddiaz Design**, licensed **CC-BY-NC-SA 4.0**
(see `assets/models/bugatti/license.txt`). This document records what the file
actually contains, because every number in `src/vehicle/presets.rs` and every
name pattern in `src/vehicle/model_rig.rs` was derived from it rather than
guessed.

## File

| property | value |
|---|---|
| format | glTF 2.0 separate (`scene.gltf` + `scene.bin` + `textures/`) |
| generator | Sketchfab 16.61.0 (originally an FBX) |
| nodes | 502 |
| meshes | 249 |
| materials | 45 |
| extensions | `KHR_materials_clearcoat`, `KHR_materials_specular`, `KHR_materials_transmission` |

## Orientation and scale

The scene root chain is `Sketchfab_model -> final_model.fbx -> RootNode`, whose
combined matrix converts the original Z-up centimetre FBX into **Y-up metres**.

World-space bounding box of the whole model:

```
min = (-1.005, -0.005, -2.248)
max = ( 1.005,  1.255,  2.223)
size =  2.010 x 1.260 x 4.472   (W x H x L)
```

The real car is 1.998 x 1.204 x 4.462 m, so **the model is 1:1 in metres** and
needs no scaling. The origin sits on the ground plane, laterally centred and
between the axles.

Axes, as authored:

```
+X = the car's LEFT      +Y = up      +Z = FORWARD (the nose)
```

`+X` is confirmed as the left side by the artist's own node names (`rotorlf` is
at `x = +0.876`) and by the steering wheel, which sits at `x = +0.388` - the
correct place for a left hand drive Veyron.

Bevy's convention is `-Z` forward, so the visual model hangs off the rigid body
under a 180 degree yaw. Converting a point between the two frames is therefore
just `(x, y, z) -> (-x, y, -z)`, which is what `physics::model_space` does.

## Measured wheel geometry

Centres are taken from the tyre meshes, which are the cleanest indicator of the
hub position. Values are in the model's own frame.

| corner | node group | tyre node | centre (x, y, z) | radius |
|---|---|---|---|---|
| front left  | `bug_veyron_09_s_wheel_wheel_*` (no suffix) | `polySurface482` | (+0.864, 0.341, +1.290) | 0.345 |
| front right | `bug_veyron_09_s_wheel_wheel_*1`           | `polySurface481` | (-0.864, 0.341, +1.290) | 0.345 |
| rear left   | `bug_veyron_09_s_wheel_wheel_*2`           | `polySurface480` | (+0.808, 0.383, -1.414) | 0.358 |
| rear right  | `bug_veyron_09_s_wheel_wheel_*3`           | `polySurface479` | (-0.808, 0.383, -1.414) | 0.358 |

Derived:

| quantity | model | real car |
|---|---|---|
| wheelbase | 2.707 m | 2.710 m |
| front track | 1.727 m | 1.715 m |
| rear track | 1.615 m | 1.618 m |
| front tyre radius | 0.345 m | 0.340 m (265/680 R500A) |
| rear tyre radius | 0.358 m | 0.355 m (365/710 R540A) |

These are the values that appear in `presets.rs`.

## Node groups the rig cares about

| slot | matched by | notes |
|---|---|---|
| rim | prefix `bug_veyron_09_s_wheel_wheel_`, trailing index selects the corner | spins |
| tyre | `polySurface479`..`polySurface482` | spins |
| brake disc | `rotorlf` / `rotorrf` / `rotorlr` / `rotorrr` | spins |
| brake caliper | `caliperlf` / `caliperrf` / `caliperlr` / `caliperrr` | steers and follows the suspension, **does not** spin |
| steering wheel | contains `steering_wheel` | rotates about the column |
| everything else | - | stays on the body |

## The rear light clusters

Matched separately, by `lights::classify`, and never re-parented - they are part
of the body. Centres are in the model's own frame, so the rear of the car is at
negative `z`.

| lamp | node | centre (x, y, z) | size (m) | material |
|---|---|---|---|---|
| left lens | `taillightL_detail_glass_red` | (+0.478, 0.790, -2.085) | 0.30 x 0.13 x 0.17 | `detail_glass_red` |
| left element | `taillightL_tail_light` | (+0.477, 0.791, -2.032) | 0.30 x 0.13 x 0.14 | `lights_chrome` |
| right lens | `taillightR_detail_glass_red` | (-0.478, 0.790, -2.085) | 0.30 x 0.13 x 0.17 | `detail_glass_red` |
| right element | `taillightR_tail_light` | (-0.477, 0.791, -2.032) | 0.30 x 0.13 x 0.14 | `lights_chrome` |
| centre strip | `body_detail_glass_red` | (0.000, 0.888, -2.101) | 0.43 x 0.02 x 0.03 | `detail_glass_red` |

Three traps in these names, which is why the matching is as narrow as it is:

* `detail_glass_red` is also worn by `glassLTL` / `glassRTL`, which sound like
  part of the tail light but sit at (±0.863, 0.592, -1.936) - the red side
  markers on the rear wings. They do not light up with the brakes.
* `lights_chrome` is shared with both headlights, the mirror shells and several
  body reflectors, so the material has to be cloned per lamp surface rather than
  edited in place.
* Inside each cluster, `reverse_light`, `textured_reflector`, `lights_chrome`,
  `black` and `detail_glass_clear` are all named after the same group and all
  stay dark.

## Pivots

Every wheel node in this file has its transform baked so the pivot is at the
**model origin**, not at the hub - a `-99.99999 ... 0.01` matrix pair left over
from the FBX centimetre conversion. Rotating those nodes directly would swing
the wheels around the middle of the car.

`model_rig.rs` therefore:

1. measures the hub centre of each corner from the mesh bounding boxes it
   actually found (so it does not depend on the numbers in this table),
2. creates a **hub pivot** and a child **spin pivot** at that measured centre,
3. re-parents each node group under the right pivot with its transform rebased
   into the pivot's frame.

The physical wheel positions come from `VehicleConfig::wheels` and are
independent of all of this; the rig only decides where the pictures go, and logs
a warning if the two disagree by more than 0.2 m.
