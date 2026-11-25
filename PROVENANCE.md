# Provenance: what is in this tree and where it came from

Base: `fxceless/victory-fork` at `f85101b`, then the waves below. Authorship is kept in the
commit history. Some changes were adapted by hand or only partly taken to fit this tree.

- **Wave 1A** (19 PRs, fxceless/victory-fork): #615, #657, #734, #735, #749, #756, #757, #758,
  #759, #788, #789, #799, #806, #808, #837, #841, #846, #854, #858.
- **Wave 1B** (37 PRs, fxceless/victory-fork): #510, #604, #644, #715, #753, #754, #763, #764,
  #765, #768, #783, #784, #785, #787, #792, #795, #802, #803, #804, #805, #807, #809, #812,
  #824, #827, #836, #838, #839, #845, #853, #855, #856, #857, #860, #862, #864, #868.
- **Wave 2**: an aggregate import of the vitutu1/victory-perf work (batching, copy-back and
  barrier work, shader and compute pipeline precompile, lighting and exposure fallbacks).
- **Wave 3** (14 PRs, Marvisak/VIctory): #240, #443, #478, #1033, #1537, #1623, #1880, #1894,
  #1955, #1956, #2007, #2012, #2037, #2102 (#1873 was already covered).
- Release follow-ups: scalar unspill tracking (`3d13942`) and VOP3 no-op literals (`0dbf043`).

## Tried and left out (with reasons)

A later round was built and measured on an RTX 3080 Ti: speed switches `VICTORY_ASYNC_SUBMIT`,
`VICTORY_QUEUE2`, `VICTORY_JOIN_COPIES` gave no gain beyond noise (about 5 %) in the first-room
scene, and several crashed in earlier runs. A combined set of five more PRs (fork #1043, #685,
#1059, #1044, VIctory #2654) made that scene 26 % slower in the same test, so it is not merged.
Open candidates still to evaluate: VIctory #2349, #2547, #2211, #2614, #1653, victory-perf #11,
fork #363. Method and numbers are kept with the project notes.
