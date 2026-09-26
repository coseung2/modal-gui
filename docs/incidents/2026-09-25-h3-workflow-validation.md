# MiniMax H3 workflow validation failure

- Date/time: 2026-09-25, Asia/Seoul
- Impact: New FL2V/Ref2V H3 generations did not reach inference. No new H3 credits were spent after the license gate failure.

## Symptoms and evidence

- The deployed Modal app `minimax-h3-latest-workflows` reached CUDA startup, then returned HTTP 400 from ComfyUI `/prompt`.
- Validation errors reported missing inputs for `UNETLoader`, `MiniMaxH3SigmaShift`, and `MinimaxH3LatentUpscaler3D`.
- Other attempts reported missing `VHS_VideoCombine`, `MinimaxH3LatentUpscaler3D`, and `BlockSparseAttention` custom nodes.
- The local `- Modal L40S` workflow files differ from their originals only in preview-node mode and hidden preview metadata; model, resolution, and length settings are unchanged.
- A previously completed T2V artifact is present in the Modal volume and was copied to `F:\modal-gui\h3-clips\existing` for pipeline work.
- A new remote call was stopped by the deployed MiniMax H3 license gate because the required authorization attestation was absent. The remote exception explicitly states that the attestation is only valid when the required authorization has been obtained.

## Confirmed cause and unresolved items

- Confirmed: the deployed worker's UI-to-API conversion drops widget inputs and the deployed ComfyUI image lacks custom nodes required by the supplied workflows.
- Confirmed: the supplied L40S variants do not contain substantive L40S execution changes.
- Resolved in source: the deployed worker was recovered from `C:\Users\coseung2\Desktop\modal_h3_latest_worker.py`, corrected, and moved into `modal/app.py`.
- Deployment verification: Modal app `minimax-h3-latest-workflows` version `v14`, tag `workflow-api-fix-20260925`, deployed at 2026-09-25 05:57:57 Asia/Seoul.
- Remaining runtime verification: a GPU generation has not yet been run against v14 because the required license attestation is absent.
- The existing authorized test path passed the attestation directly to the Modal method; the local wrapper was incorrectly requiring an environment variable. The wrapper now preserves the existing direct-argument path.

## Response and verification

- Added a local converter that preserves `widgets_values_named` and link inputs when producing a ComfyUI API prompt.
- Deployed v14 with the same named-widget mapping, corrected `MinimaxH3LatentUpscaler3D` field names, normalized `/data/input` paths, and explicit T2V/Ref2V workflow selection.
- Updated the local worker to support T2V jobs without an image, configurable clip dimensions, and an output root such as `F:\modal-gui`.
- Fixed the Windows worker launch path and the worker test discovery pattern.
- Verified the four prepared API prompts contain the previously missing loader, sigma-shift, and upscaler inputs.
- v15 verified FL2V on L40S after disabling incompatible Sage/Chunk/Sparse optimization kernels and preserving model/conditioning passthroughs.
- v17 verified Ref2V on L40S after adding the Deno custom-node pack and completing preview/text-encoder passthroughs.
- YuE2 was installed in a separate L40S Modal worker; the first 209.7-second 48 kHz stereo track completed with `m-a-p/YuE2-3B` and `m-a-p/YuE2-Vae`.
- The final trailer render was verified at 1920x1080, 24 fps, 1440 frames, 48 kHz stereo AAC, and exactly 60 seconds.
- `npm run check:worker` passes 3 tests; `npm run build` passes; Python compilation and `git diff --check` pass.

## Follow-up

- Keep the v17 L40S image pinned for later workflow changes and retain the source clips, prepared workflows, patch-note source, YuE2 artifacts, and final export under `F:\modal-gui`.
