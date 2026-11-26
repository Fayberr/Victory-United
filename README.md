# VI-BR-PORT: a versão para placas de 8 GB

Camada de compatibilidade que roda no Windows, de forma nativa, a versão de
**Xbox Series X|S** (plataforma *Scarlett*, jogos feitos com o **GDK**) de um
título, a partir dos arquivos do próprio usuário. O código x86-64 do console
roda sem alteração dentro do nosso processo; por baixo dele ficam as bibliotecas
do sistema do console reescritas (HLE) e um back-end Direct3D 12 que recompila
os shaders da GPU do console (GFX10/RDNA2) para DXIL.

Este fork tem um alvo só: **placa NVIDIA de 8 GB com ondas de 32 lanes**
(testado numa RTX 4060 Ti) ligada a um **monitor 1080p**.

**Este repositório não contém arquivos, código ou recursos do jogo.** É preciso
ter a sua própria cópia. Nada aqui tem ligação com o desenvolvedor ou a
editora do jogo, nem é aprovado por eles.

## Onde está (2026-11-26)

A base foi trocada por uma que, na mesma placa de 8 GB, mostra rostos, pele,
cabelo e luz bem melhores. Por cima dela vieram os ajustes para NVIDIA de 8 GB.

| | Medido (RTX 4060 Ti 8 GB, i7-13700K, 1080p) |
|---|---|
| Imagem, perfil Series X | rostos e pele com textura, luz coerente nas cutscenes |
| fps, perfil Series X, release | ~5,6 (a placa sozinha leva ~105 ms por quadro) |
| Memória da placa, Series X | no limite; na prisão passa do orçamento e a textura borra |
| Perfil Series S, release, cutscene de abertura | imagem certa, boca animada, placa em 5,2 de 6,8 GB, ~18–19 fps (vídeo de abertura: 30 fps) |
| Perfil Series S, release, prisão (jogável, ~430 s depois do início) | ~14 fps, luz limpa com `VICTORY_AMBIENT=constant:0.1` |

**Ainda não está jogável.** O alvo é ter 15 fps ou mais, com imagem nítida, nas cutscenes e na prisão. Use o perfil Series S.

## O que este fork acrescenta

- **`VICTORY_RT_MISS=1`**: o teste de raio não acerta nada; sem a estrutura de ray
  tracing montada, as sombras de luzes locais saíam de lixo (rosto preto ou com "purpurina").
- **Skinning do ray tracing pulado** com `RT_MISS=1`: os 30 programas `CSComputeSkinning*`
  (`bvh_skinning`) refaziam a pose de todo personagem só para a estrutura de ray tracing,
  que ninguém usa assim, e eram a maior parte do tempo da placa (~6,8 → ~10,9 fps no
  Series S). `VICTORY_RT_SKIN=1` volta a rodá-los.
- **Assado de decoração do rosto reconhecido pelo nome** (`PS_DecorationBlend_*`,
  `PS_BakeDecorations_*`, 51 programas): a lista de 4 hashes deixava de fora o que
  faz os rostos da cutscene, e o rosto saía preto.
- **Listas de memória alterada compactadas em vez de apagadas** quando enchem: apagar
  deixava texturas com bytes velhos depois de a memória mudar.
- **Composição de planos do present** (Series S: o mundo e a interface em planos separados),
  enviada à frente como o present de um plano só, sem esperar a placa.
- **Boca e expressões dos rostos**: a rede que anima os rostos guarda os valores em buffers
  de 16 bits e a tradução os lia como zero nas tabelas de descritores; agora são movidos
  como inteiros de 16 bits e a boca abre e fecha nas falas.
- **Sombras em cascata exatas em placas de 32 lanes** (NVIDIA) e **views de textura
  escritas pela placa** copiadas da própria textura.
- **Janela "Brazil | VIctory"** com a logo do fork (`VICTORY_ICON`) e
  **`VICTORY_KEYS_FILE`**: uma tecla escrita num arquivo é apertada no jogo sem precisar
  de foco (para scripts que passam pelos termos e pelo seletor de conta).
- **Vídeo da abertura sem voltar quadros**: cada lista tem o seu próprio buffer de upload
  para os planos de vídeo escritos pela CPU; com vários quadros em voo, um quadro
  sobrescrevia o anterior e os letreiros da abertura voltavam no meio do fade.
- **Textura nova só aparece depois do upload** (`VICTORY_SHOW_AFTER`, agora ligado; `0`
  desliga): um pedaço reaproveitado do pool mostrava bytes velhos, e o banco do carro
  da abertura virava um quadriculado verde e azul.
- **Patches do título também no release** (`VICTORY_UPSCALER=6`: o upscaler do próprio
  jogo no Series S, já que os passes do FidelityFX não têm DXIL aqui).
- **Pipelines**: pilha de 64 MB para o compilador do driver da NVIDIA, compute começando
  na criação, espera de 500 ms só nos 60 primeiros quadros (8 ms depois) e uma pipeline
  atrasada segurando só o primeiro quadro que a pede.
- **wave64 em placas de 32 lanes**: programas que pediriam ondas de 64 lanes usam a forma larga.
- **DirectStorage** grava em páginas somente leitura como o motor do console.
- `VICTORY_NO_WATCHDOG`, `VICTORY_NULL_MAGENTA` (diagnóstico) e o teto de 262.144
  faixas remapeadas.

## Como rodar

```powershell
copy victory.example.toml victory.toml      # e aponte game_dir para a sua cópia
cargo build --locked --release -p victory
target\release\victory.exe shaders <os arquivos de shaders do jogo>
target\release\victory.exe run <o executável do jogo> --console series-s --execute
```

Configuração usada nas medições acima: `VICTORY_RT_MISS=1`, `VICTORY_RELAY=2`, `VICTORY_CARD_STEPS=2`,
`VICTORY_THIN=0`, `VICTORY_DISPLAY_MODE=1920x1080`, `VICTORY_PIPELINE_BOOK=1`,
`VICTORY_MEMORY_CARD_MB=1024`, `VICTORY_UPSCALER=6` (Series S). Não ligue
`VICTORY_THIN=1` numa NVIDIA (o fps cai a menos da metade).
`VICTORY_AMBIENT=constant:0.1`: sem raios, as sondas de luz indireta deixam blocos
coloridos nas paredes (a mancha roxa da cela); uma luz indireta constante e fraca tira
os blocos e mantém a noite escura. A estimativa (`VICTORY_AMBIENT=<número>`) posteriza
paredes e pele, e níveis constantes altos deixam a noite clara como dia.

## Licença

GPL-3.0-or-later, como o VIctory. Ver [LICENSE](LICENSE).
