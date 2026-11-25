# Saves

Story progress lives in the data folder, not in the game files, and each data folder keeps its
own. To continue someone else's progress or your own from another platform, that save has to be
converted into this build's format first.

## What a save looks like here

```
<data_dir>\7952987A\saves\0009000000000001\00000000-0000-0000-0000-00007952987a\
├─ Player~333dc56ddffd8ea0\                 profile (leave alone)
├─ SettingsProfileSettings~0b4e25fca06268d0\
└─ SARC0015~f3ad35e1a05a7da0\               one story save (SARC0015 is the autosave slot)
   └─ v1\
      ├─ 0       save data (ASAV header, LZ4-packed JSAV body)
      ├─ 1       save header (296 bytes)
      └─ manifest
```

Keep the game closed and the data folder backed up while working on a save.

## Importing a converted save

PS5 and Xbox story saves hold the same body. The game only encrypts its own on the way out, and
it decides that with one function. `VICTORY_PATCH` makes that function answer "no" in the loaded
copy, so an unencrypted save loads:

```
set VICTORY_PATCH=0x28d0d60=31c0c3:ffc9f7c1
```

The patch is written only when the bytes at that address still are `ff c9 f7 c1`, so a different
game build is refused with a message instead of being half-patched. A run with it on also refuses
to write encrypted saves, hence the switch.

Tools that do the conversion, and their terms:

- [ztrx-ps5-save-import](https://github.com/LilBigJP/ztrx-ps5-save-import) (GPL-3.0), for saves
  already decrypted on a PS5. It brings a `VICTORY_PATCH` of its own, so for this tree only its
  `tools/` converter and template are used, not its patch. Ships an example save (Chapter 4, open
  world).
- [save-relay](https://github.com/lilman514/save-relay) (GPL-2.0), a converter around the same
  format from the XCL work; it writes a whole profile of its own.

Both expect the decrypted `chives.dat` from a PS5 save tool (GarlicSaves), not a raw PS5 image.

Write the imported slot as the next free `SARCnnnn` and leave the existing slots alone, or the
game may pick another. Check the checkpoint in game and make a manual save in a new slot once it
loads.
