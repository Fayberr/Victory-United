# Port provenance

The Streamline integration in Victory United is derived from
[whiteh4cker-tr/VIctory_sm86](https://github.com/whiteh4cker-tr/VIctory_sm86)
under GPL-3.0-or-later. Copyright remains with the original authors. See [LICENSE](LICENSE)
and [credits](docs/credits.md).

| Area | Source |
| --- | --- |
| Streamline Super Resolution ABI, camera constants and evaluation | readmet3xt-lab in `8756f6ac5`, part of `9296e0329` |
| FidelityFX entry and Series X profile | readmet3xt-lab in `88e372cff` and `ab2ba97e9`, building on DexNom/VIctory; this branch uses Victory United's existing FidelityFX bridge and profile |
| Frame generation presentation and scene capture | readmet3xt-lab in `8756f6ac5`, `2abeba3b9` and `1d3040027`, merged in `11509be9d` |
| Ampere and Turing add-on loader and CUDA adapter selection | Alp Eren Demir (whiteh4cker-tr) in `b4686122a` |
| Setup reference | readmet3xt-lab in `b4da68cf4` |

The NVIDIA runtime libraries and the optional dlssg_for_sm86 add-on are
supplied by the user. NVIDIA retains rights to its libraries and kernels.
