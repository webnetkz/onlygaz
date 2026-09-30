# Optimized roadside assets

The runtime lamp GLB was derived from the user-supplied Sketchfab archive. Its
accompanying license file is retained verbatim.

- Guardrails use the original procedural merged beam-and-post mesh generated
  by the game. No external guardrail model is loaded at runtime.
- `bridge_street_light.glb` — based on “Bridge street light 3” by Streetlights
  & ETC, CC BY 4.0. The runtime version is centered, welded, simplified,
  palette-merged where possible, and pruned. It is shared by every lamp.

Each guardrail chunk is one low-cost mesh with a shared metal material. Street
lights share one scene asset. Collision remains separate continuous geometry,
so visuals do not add triangle-mesh physics cost or collision seams.
