# dsh-ai-cursor · AI 鼠标

[English](README.md) | [中文](README-zh.md) | [Español](README-es.md) | [हिन्दी](README-hi.md) | Português

`dsh-ai-cursor` dá a um agente do DeepSeek Harness o seu próprio segundo cursor na área de trabalho do Windows: uma sobreposição que ignora cliques, com o selo `AI 鼠标` e uma pequena baleia, além de um painel de controle flutuante onde uma pessoa pode acompanhar o trabalho, mudar a aparência, pausar e limitar o que o agente pode fazer. Ele nunca move o ponteiro real do mouse e nunca toma o foco do teclado.

> **Por quê:** um agente que opera a sua área de trabalho divide um único ponteiro com você. Você não vê o que ele está prestes a tocar, e o seu próprio movimento de mouse cai no meio do clique dele. Um segundo cursor, do próprio agente, torna o agente visível sem sequestrar o seu ponteiro.

## O que você recebe

Duas janelas em camadas do Win32 dentro de um único processo Rust (`ai-mouse-overlay.exe`), comandadas por um shell de plugin TypeScript/Cordis sobre um protocolo de linhas JSON em stdin/stdout.

| | `AiMouseOverlay` — o cursor da IA | `AiMousePanel` — o painel de controle |
|---|---|---|
| Estilos estendidos | `WS_EX_LAYERED \| WS_EX_TRANSPARENT \| WS_EX_TOOLWINDOW \| WS_EX_NOACTIVATE \| WS_EX_TOPMOST` | `WS_EX_LAYERED \| WS_EX_TOOLWINDOW \| WS_EX_NOACTIVATE \| WS_EX_TOPMOST` |
| Mouse | Clique atravessa: os cliques chegam à janela de baixo; nunca toma o foco | Não atravessa: testa os próprios widgets; clicar nunca rouba o foco (`WS_EX_NOACTIVATE`) |
| Tamanho | 360×240 px, ponta desenhada em (200, 44) | 452×396 px aberto, 132×40 px como chip |
| Pintura | GDI, supersampling 4×, reduzido, enviado com `UpdateLayeredWindow` | GDI, supersampling 2×, mesmo caminho de envio |
| Captura de tela | Excluído por padrão (`WDA_EXCLUDEFROMCAPTURE`), alternável | Igual |

- `ai_mouse_move` coloca a ponta exatamente no ponto pedido: as coordenadas são pixels físicos absolutos e o processo é DPI-aware por monitor (v2).
- O painel aparece na primeira vez que a sobreposição é usada (config `panel`), abre perto da borda direita da tela, centralizado na vertical, e é arrastável pelo cabeçalho.
- O painel concentra toda a superfície de controle: 显示 / 隐藏 / 停止 / 启用, o interruptor do selo `AI 提示`, um controle de opacidade (15 %–100 %), oito amostras de cor com leitura `#RRGGBB` ao vivo, o desenho 鲸鱼 / 圆点 / 箭头, um controle de tamanho (60 %–200 %), a escada de permissões 停止 / 只读 / 操作 / 管理员, e um rodapé com visibilidade, coordenadas, fps e o nível atual.
- O cursor some sozinho após `idleMs` sem movimento (60 s por padrão, `0` = nunca). O painel fica onde está.
- Um processo de sobreposição por host do DSH: sobe na primeira chamada de ferramenta e encerra quando o plugin é descarregado.

### Sobreposição ≠ entrada

Este plugin nunca sintetiza cliques nem move o ponteiro do sistema; ele apenas mostra onde o agente está olhando. A entrada real continua passando pela sua ferramenta de computer-use: prefira um modo de entrega em segundo plano quando a ferramenta tiver um (o `delivery_mode: "background"` do Cua Driver), e os dois cursores param de brigar.

## Layout do painel

Painel aberto, 452×396. Os rótulos são as strings em chinês que a sobreposição realmente desenha:

```text
┌────────────────────────────────────────────┐
│ ◉ AI 鼠标                        ─     ✕   │
├────────────────────────────────────────────┤
│ [ 显示 ][ 隐藏 ][ 停止 ][ 启用 ]            │
├────────────────────────────────────────────┤
│ AI 提示            [开][关]     透明度 70%  │
│ ━━━━━━━━━━━●━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ │
│ 颜色  ● ● ● ● ● ● ● ●            #4FC3F7   │
├────────────────────────────────────────────┤
│ 造型   [鲸鱼][圆点][箭头]       大小 100%   │
│ ━━━━━━━●━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ │
├────────────────────────────────────────────┤
│ 权限等级 [停止][只读][操作][管理员]         │
├────────────────────────────────────────────┤
│ 可见 · (900,520) · fps 38 · 权限 管理员     │
└────────────────────────────────────────────┘
```

Os números deste esboço (fps, coordenadas, porcentagens) são valores de exemplo ilustrativos, não medições.

- `显示` / `隐藏` — mostrar ou ocultar o cursor da IA.
- `停止` / `启用` — 停止 leva o nível de permissão a 0 e oculta o cursor; 启用 restaura o nível anterior, no mínimo 2.
- `AI 提示` — o selo sob o cursor, ligado e desligado com `开` / `关`.
- `透明度` — opacidade do cursor e do selo, de 15% a 100%.
- `颜色` — oito amostras mais o `#RRGGBB` atual; a cor de destaque pinta o cursor, a borda do selo e os realces do painel.
- `造型` — `鲸鱼` (baleia, o padrão), `圆点` (ponto) ou `箭头` (seta).
- `大小` — tamanho do cursor, de 60% a 200%.
- `权限等级` — `停止` (0), `只读` (1), `操作` (2), `管理员` (3).
- Rodapé — visível ou oculto, coordenadas do cursor, quadros por segundo e o nível de permissão atual.

Notas:

- O cabeçalho inteiro é a alça de arraste, e o painel fica onde você o solta.
- Recolher é só com o mouse: clique em `─` ou em `✕` no cabeçalho e o painel encolhe para o chip de 132×40 (`≡ AI 鼠标`); clicar no chip abre de novo. A ativação por teclado não é suportada de propósito. O equivalente por ferramenta é `ai_mouse_panel { expanded: false }`, e `expanded: true` reabre; não há botão de ocultar — ocultar é `ai_mouse_panel { visible: false }`.
- Uma única função de layout produz os retângulos desenhados e a lista de acertos, então o clique cai no que você vê.
- O painel nunca toma o foco: o seu teclado fica no aplicativo em primeiro plano.

## Níveis de permissão

Os nomes são os que o painel imprime: 停止 (parado), 只读 (somente leitura), 操作 (conduzir), 管理员 (administrador).

| Nível | Nome | O que a IA pode fazer |
|---|---|---|
| 0 | 停止 | Nada além do estado e do encanamento de sistema de captura/inatividade: `ai_mouse_status` e `ai_mouse_capture_visible` continuam funcionando. |
| 1 | 只读 | Mostrar ou ocultar o cursor e o painel. |
| 2 | 操作 | O nível 1 mais `ai_mouse_move`. |
| 3 | 管理员 | O nível 2 mais a aparência (`label`, `theme`, `badge`, `opacity`, `color`, `scale`) e baixar o nível. |

- Uma pessoa pode definir qualquer nível no painel. O agente tem uma única alavanca, `ai_mouse_permission`, e ela só desce: um nível acima do atual é recusado.
- Dois portões independentes: a camada TypeScript confere o nível mínimo de cada ferramenta e o processo Rust confere `min_perm` de novo para cada comando. Um cache desatualizado do lado do agente não escala nada.
- 停止 significa nível 0 mais cursor oculto. 启用 restaura `max(nível anterior, 2)`.
- `ai_mouse_stop` é uma porta de mão única: oculta o cursor, baixa o nível para 0 e mostra o painel para a pessoa clicar em 启用.
- Uma recusa nomeia a ação, o nível necessário, o nível atual e diz que só uma pessoa pode subir; um agente que lê isso não tem motivo para tentar de novo.

## Arquitetura

O shell do plugin registra as ferramentas; a sobreposição é um pequeno programa Rust à parte que possui as duas janelas. Eles conversam por stdin/stdout, um objeto JSON por linha.

```text
DSH host process (Node / Cordis)
  src/index.ts        registers the ai_mouse_* tools, holds the permission gate
        │
        │  JSON line protocol, one object per line, stdin / stdout
        ▼
ai-mouse-overlay.exe (Rust / Win32)
  AiMouseOverlay      layered, click-through, excluded from screen capture
  AiMousePanel        layered, interactive, collapsible
```

- `src/index.ts` — plugin Cordis: registra as catorze ferramentas, filtra cada chamada e aplica a aparência configurada no primeiro uso.
- `src/overlay.ts` — dono do processo filho e do protocolo de linhas: ids de requisição, 3 s de espera por resposta, 5 s de inicialização, `quit` ao descarregar.
- `src/config.ts` — o esquema Schemastery; o host valida no carregamento.
- `src/permissions.ts` — a tabela de níveis e as funções puras, testadas sem janela.
- `overlay/src/main.rs` — Win32: duas janelas em camadas, pintura GDI com supersampling, teste de acerto do painel, arraste, controles e amostras.

As notas de projeto completas (em chinês) estão em [`docs/DESIGN.md`](docs/DESIGN.md). Toda resposta repete o `id` da requisição e `status` devolve o estado completo.

## Requisitos

| Item | Requisito |
|---|---|
| Sistema operacional | Windows 10 versão 2004 ou mais novo. A sobreposição precisa de `WDA_EXCLUDEFROMCAPTURE`; compilações mais antigas não conseguem excluir uma janela da captura. |
| Node.js | `^22.19.0 \|\| >=24.0.0` |
| pnpm | 11 ou mais novo, só para compilar a partir do código-fonte |
| Rust | Uma toolchain estável com o alvo MSVC, só para compilar a sobreposição a partir do código-fonte |
| Harness | Uma compilação do DeepSeek Harness com o carregador de plugins Cordis; a faixa suportada de `@deepseek-ai/dsh-tools` está declarada no `package.json` |

**Somente Windows.** macOS e Linux não estão implementados: a sobreposição é um programa Win32 sem alternativa, então nessas plataformas não há com o que desenhar um cursor.

## Instalação

Instale o pacote publicado, ou link um checkout local durante o desenvolvimento. Troque `desktop` pelo nome do seu perfil.

```sh
dsh plugin --profile desktop add dsh-ai-cursor
dsh plugin --profile desktop add link:/absolute/path/to/dsh-ai-cursor
```

Reinicie o harness depois: plugins são carregados na inicialização, então um plugin novo só aparece após um reinício. Pacotes publicados incluem `bin/ai-mouse-overlay.exe`; um checkout local precisa compilá-lo uma vez com `pnpm run build:overlay`.

## Compilação

```sh
pnpm install
pnpm run build:overlay
pnpm run typecheck
pnpm test
pnpm run build
```

- `pnpm install` — dependências de desenvolvimento de TypeScript (pnpm 11; o script de build do `esbuild` está liberado no `pnpm-workspace.yaml`).
- `pnpm run build:overlay` — `cargo build --release` em `overlay/` e depois copia `ai-mouse-overlay.exe` para `bin/`. Se o `cargo` falhar e já existir um binário em `bin/`, esse binário é mantido.
- `pnpm run typecheck` — `tsc --noEmit`.
- `pnpm test` — `vitest run`.
- `pnpm run build` — `tsdown` empacota `lib/index.js` e `lib/index.d.ts`.

Ou pressione `Ctrl+Shift+B` no VS Code e rode a tarefa `all (install → overlay → check → build)`, que encadeia os mesmos cinco passos. Compilar a sobreposição exige Rust estável com as ferramentas de build do MSVC (`x86_64-pc-windows-msvc`); só o shell do plugin precisa de Node 22+.

## Configuração

Todas as chaves são declaradas com Schemastery em `src/config.ts`; o host valida no carregamento e recusa valores inválidos.

| Chave | Padrão | Significado |
|---|---|---|
| `enabled` | `true` | Registrar a família de ferramentas. |
| `exePath` | `""` | Caminho absoluto para `ai-mouse-overlay.exe`. Vazio resolve o `bin/` incluído. |
| `label` | `"AI 鼠标"` | Texto do selo sob o cursor. |
| `theme` | `"whale"` | Desenho: `whale`, `dot` ou `arrow`. |
| `idleMs` | `60000` | Ocultar o cursor após esses ms sem movimento; `0` nunca oculta. |
| `captureVisible` | `false` | Permitir que o cursor apareça em capturas e gravações. |
| `panel` | `true` | Mostrar o painel flutuante na primeira vez que a sobreposição é usada. |
| `panelExpanded` | `true` | Abrir como barra de menu completa; `false` começa como chip. |
| `opacity` | `1` | Opacidade do cursor e do selo, 0.15–1. |
| `color` | `"#4FC3F7"` | Destaque `#RRGGBB` do cursor, da borda do selo e dos realces do painel. |
| `scale` | `1` | Multiplicador de tamanho do cursor, 0.6–2. |
| `badge` | `true` | Mostrar o selo `AI 提示`. |
| `permission` | `3` | Teto inicial: 0 停止, 1 只读, 2 操作, 3 管理员. |

`permission` é um ponto de partida, não um cadeado: ele é aplicado na primeira chamada de ferramenta e, daí em diante, o painel manda. Um agente pode baixá-lo; só uma pessoa pode subir.

## Ferramentas

Catorze ferramentas. "Permissão mínima" é o nível mais baixo em que a chamada é permitida; `0 · any` também funciona com o cursor parado.

| Ferramenta | Permissão mínima | O que faz |
|---|---|---|
| `ai_mouse_show` | 1 · 只读 | Mostra o cursor, aplicando a aparência configurada na primeira vez. |
| `ai_mouse_hide` | 1 · 只读 | Oculta o cursor. |
| `ai_mouse_move` | 2 · 操作 | Desliza até coordenadas absolutas em pixels do dispositivo (`x`, `y`, `duration_ms?`; padrão 320, `0` salta). Mostra o cursor. |
| `ai_mouse_panel` | 1 · 只读 | Mostra ou oculta, expande ou recolhe o painel (`visible?`, `expanded?`). |
| `ai_mouse_status` | 0 · any | Lê visibilidade, posição, rótulo, desenho, tamanho, opacidade, cor, selo, permissão, estado do painel e fps. |
| `ai_mouse_capture_visible` | 0 · any | Permite que o cursor apareça em capturas (`enabled`); desligado por padrão. |
| `ai_mouse_label` | 3 · 管理员 | Define o texto do selo (`text`); string vazia oculta o selo. |
| `ai_mouse_theme` | 3 · 管理员 | Desenho: `whale` (padrão), `dot`, `arrow`. |
| `ai_mouse_badge` | 3 · 管理员 | Mostra ou oculta o selo `AI 提示` (`visible`). |
| `ai_mouse_opacity` | 3 · 管理员 | Opacidade do cursor e do selo, 0.15–1. |
| `ai_mouse_color` | 3 · 管理员 | Cor de destaque como `#RRGGBB`. |
| `ai_mouse_scale` | 3 · 管理员 | Tamanho do cursor, 0.6–2. |
| `ai_mouse_permission` | 3 · 管理员 | Lê ou baixa o nível (`level?`, 0–3). Subir é recusado. |
| `ai_mouse_stop` | 3 · 管理员 | Oculta o cursor, baixa para o nível 0 e mostra o painel. Só uma pessoa pode religar. |

- `ai_mouse_permission` sem `level` apenas lê, mas a chamada em si precisa do nível 3; de baixo, leia o nível com `ai_mouse_status`.
- Toda ferramenta devolve uma linha de texto; uma chamada barrada devolve a frase de recusa em vez de fazer algo.
- `ai_mouse_capture_visible` relê o estado até as janelas recriadas informarem o valor novo, então a resposta reflete o estado real.
- Fluxo típico: `ai_mouse_move` até o alvo, clicar com a ferramenta de computer-use e então `ai_mouse_hide` — ou deixar o cursor visível e o tempo de inatividade ocultá-lo.

## Protocolo de linha

Um objeto JSON por linha em stdin; uma resposta por linha em stdout com o mesmo `id`. Os comandos `system` são encanamento que o shell do plugin envia sozinho e não são acessíveis por ferramenta.

| Comando | Permissão exigida | Efeito |
|---|---|---|
| `{"cmd":"show"}` / `{"cmd":"hide"}` | ≥ 1 | Mostrar ou ocultar o cursor. |
| `{"cmd":"move","x":900,"y":520,"duration_ms":320}` | ≥ 2 | Deslizar até coordenadas absolutas da tela. |
| `{"cmd":"panel","visible":true}` | ≥ 1 | Mostrar ou ocultar o painel. |
| `{"cmd":"panel_expanded","expanded":true}` | ≥ 1 | Barra de menu completa, ou o chip. |
| `{"cmd":"opacity","value":0.7}` | ≥ 3 | Opacidade do cursor e do selo, 0.15–1. |
| `{"cmd":"color","hex":"#4FC3F7"}` | ≥ 3 | Cor de destaque. |
| `{"cmd":"scale","value":1.2}` | ≥ 3 | Tamanho do cursor, 0.6–2. |
| `{"cmd":"badge","visible":true}` | ≥ 3 | `AI 提示` ligado ou desligado. |
| `{"cmd":"theme","theme":"whale"}` | ≥ 3 | `whale`, `dot` ou `arrow`. |
| `{"cmd":"label","text":"AI 鼠标"}` | ≥ 3 | Texto do selo. |
| `{"cmd":"permission","level":2}` | ≥ 3, só descer | Nível de permissão. |
| `{"cmd":"pause"}` / `{"cmd":"resume"}` | ≥ 3 | O mesmo que 停止 / 启用. |
| `{"cmd":"idle","idle_ms":60000}` | system | Ocultar após esse tempo de inatividade; `0` nunca. |
| `{"cmd":"affinity","capture_visible":false}` | system | Exclusão de captura; recria as duas janelas. |
| `{"cmd":"dump","path":"frame.bmp","target":"cursor"}` | system | Pintar um quadro em BMP (autoteste, sem captura de tela). |
| `{"cmd":"status"}` | any | Ler o estado completo. |
| `{"cmd":"quit"}` | system | Encerrar o processo da sobreposição. |

```json
{"id":"1","cmd":"move","x":900,"y":520,"duration_ms":320}
{"id":"2","cmd":"affinity","capture_visible":true}
{"id":"3","cmd":"theme","theme":"whale"}
{"id":"4","cmd":"dump","path":"frame.bmp","target":"cursor"}
{"cmd":"quit"}
```

Toda resposta é `{"id":"<mesmo id>","ok":true,…}`. Um comando recusado responde `ok:false` e um `error` que nomeia o nível necessário. `status` acrescenta `visible`, `x`, `y`, `label`, `theme`, `badge`, `opacity`, `scale`, `color`, `permission`, `panel`, `panel_expanded`, `hwnd`, `panel_hwnd`, `fps`, `capture_visible` e `excluded_from_capture`.

## Solução de problemas

### O cursor não aparece

- Confira o nível: `ai_mouse_status` imprime `permission=`. No nível 0 (停止) o cursor fica oculto de propósito; uma pessoa precisa clicar em 启用 ou num nível maior no painel.
- O cursor some sozinho após `idleMs` sem movimento (60 s por padrão). Chame `ai_mouse_show` ou `ai_mouse_move` de novo, ou use `idleMs: 0`.
- Se `ai_mouse_status` devolver um erro mencionando o executável, veja o item seguinte.
- Ocultar o painel não oculta o cursor, e ocultar o cursor não oculta o painel: são independentes.

### `ai-mouse-overlay.exe` não encontrado

- Compile uma vez: `pnpm run build:overlay` grava `bin/ai-mouse-overlay.exe`.
- Sem `exePath`, o resolvedor tenta `<pacote>/bin/ai-mouse-overlay.exe` e depois a compilação de desenvolvimento em `overlay/target/release/ai-mouse-overlay.exe`.
- `exePath` precisa ser absoluto; um caminho relativo é recusado.
- A mensagem de erro contém o caminho absoluto que ele tentou: leia antes de adivinhar.

### O modo de captura não fez efeito

- O Windows permite `SetWindowDisplayAffinity` uma única vez por janela: voltar para `WDA_NONE` falha com `ERROR_NOT_ENOUGH_MEMORY (0x80070008)`. Por isso a sobreposição recria as duas janelas sempre que o modo de captura muda, e isso tem de acontecer na thread que possui o loop de mensagens (criá-las em outra thread trava o `ShowWindow`).
- `ai_mouse_capture_visible` consulta o estado até a sobreposição informar o valor pedido; se a resposta ainda mostra o valor antigo, as janelas ainda não foram recriadas.
- Compilações antigas do Windows não têm `WDA_EXCLUDEFROMCAPTURE`, então ali a exclusão não faz nada, em silêncio.

### Permissão negada

- A recusa é deliberada: a ferramenta precisa de um nível, a sobreposição está num nível mais baixo e só uma pessoa pode subir.
- Não tente de novo com outros argumentos. A ferramenta é barrada e o comando é barrado outra vez dentro do processo.
- Peça à pessoa para clicar em 管理员 no painel — ou em 启用, se o nível for 0.
- Se uma pessoa acabou de subir o nível, uma nova tentativa funciona: o plugin relê o nível quando o valor em cache recusa uma chamada.

### O painel sumiu

- `ai_mouse_panel { visible: true }` mostra; acrescente `expanded: true` para a barra de menu completa.
- O chip recolhido tem 132×40 e fica onde o painel estava por último; as duas janelas são topmost, então ele fica acima das janelas normais.
- Com `panel: false` o painel simplesmente não aparece no primeiro uso; uma chamada de ferramenta o traz de volta.

### As ferramentas `ai_mouse_*` não existem

- `enabled: false` não registra nada.
- Plugins são carregados na inicialização: reinicie o harness depois de instalar ou atualizar.
- Confirme que o perfil em que você instalou é o perfil que está rodando.

## Como contribuir

Issues, traduções e patches são bem-vindos. Antes de abrir um pull request, rode `pnpm run typecheck` e `pnpm test`, e mantenha a invariante de permissão: um agente pode baixar o próprio nível, só uma pessoa sobe.

As strings em chinês de `overlay/src/main.rs` e os nomes de nível de `src/permissions.ts` são termos do produto — 显示 隐藏 停止 启用 只读 操作 管理员 AI 提示 造型 大小 透明度 颜色 权限等级 — não os renomeie. Os cinco READMEs são mantidos em sincronia: mude a mesma seção em todos.

## Licença

Apache License 2.0 — veja [LICENSE](LICENSE). © 2026 dsh-ai-cursor contributors.
