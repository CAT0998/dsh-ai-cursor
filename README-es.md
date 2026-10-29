# dsh-ai-cursor · AI 鼠标

[English](README.md) | [中文](README-zh.md) | Español | [हिन्दी](README-hi.md) | [Português](README-pt.md)

`dsh-ai-cursor` le da a un agente de DeepSeek Harness su propio segundo cursor en el escritorio de Windows: una superposición de clic transparente con la etiqueta `AI 鼠标` y una pequeña ballena, más un panel de control flotante donde una persona puede verlo trabajar, cambiar su aspecto, pausarlo y limitar lo que puede hacer. Nunca mueve el puntero real del ratón y nunca toma el foco del teclado.

> **Por qué:** un agente que maneja tu escritorio comparte un solo puntero contigo. No ves qué va a tocar, y tu propio movimiento del ratón cae en medio de su clic. Un segundo cursor, propiedad del agente, hace visible al agente sin secuestrar tu puntero.

## Qué obtienes

Dos ventanas en capas de Win32 dentro de un único proceso Rust (`ai-mouse-overlay.exe`), gobernadas por un shell de plugin TypeScript/Cordis sobre un protocolo de líneas JSON en stdin/stdout.

| | `AiMouseOverlay`: el cursor de la IA | `AiMousePanel`: el panel de control |
|---|---|---|
| Estilos extendidos | `WS_EX_LAYERED \| WS_EX_TRANSPARENT \| WS_EX_TOOLWINDOW \| WS_EX_NOACTIVATE \| WS_EX_TOPMOST` | `WS_EX_LAYERED \| WS_EX_TOOLWINDOW \| WS_EX_NOACTIVATE \| WS_EX_TOPMOST` |
| Ratón | Clic a través: los clics llegan a la ventana de abajo; nunca toma el foco | No es clic a través: hace hit-test de sus propios widgets; hacer clic nunca roba el foco (`WS_EX_NOACTIVATE`) |
| Tamaño | 360×240 px, punta dibujada en (200, 44) | 452×396 px desplegado, 132×40 px como chip |
| Dibujo | GDI, 4× supersample, reducido, volcado con `UpdateLayeredWindow` | GDI, 2× supersample, misma ruta de volcado |
| Captura de pantalla | Excluido por defecto (`WDA_EXCLUDEFROMCAPTURE`), se puede cambiar | Igual |

- `ai_mouse_move` coloca la punta exactamente en el punto pedido: las coordenadas son píxeles de dispositivo absolutos y el proceso tiene en cuenta el DPI por monitor (v2).
- El panel aparece la primera vez que se usa la superposición (config `panel`), se abre junto al borde derecho de la pantalla, centrado verticalmente, y se arrastra por su cabecera.
- El panel concentra toda la superficie de control: 显示 / 隐藏 / 停止 / 启用, el interruptor de la etiqueta `AI 提示`, un deslizador de opacidad (15 %–100 %), ocho muestras de color con lectura `#RRGGBB` en vivo, el dibujo 鲸鱼 / 圆点 / 箭头, un deslizador de tamaño (60 %–200 %), la escala de permisos 停止 / 只读 / 操作 / 管理员, y un pie con visibilidad, coordenadas, fps y el nivel actual.
- El cursor se oculta solo tras `idleMs` sin moverse (60 s por defecto, `0` = nunca). El panel se queda donde está.
- Un proceso de superposición por host de DSH: arranca en la primera llamada a una herramienta y se cierra cuando el plugin se descarga.

### Superposición ≠ entrada

Este plugin nunca sintetiza clics ni mueve el puntero del sistema; solo muestra dónde está mirando el agente. La entrada real sigue pasando por tu herramienta de computer-use: prefiere un modo de entrega en segundo plano si la herramienta lo tiene (el `delivery_mode: "background"` de Cua Driver), y los dos cursores dejan de pelearse.

## Diseño del panel

Panel desplegado, 452×396. Las etiquetas son las cadenas chinas que la superposición dibuja de verdad:

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

Los números de este esquema (fps, coordenadas, porcentajes) son valores de ejemplo ilustrativos, no mediciones.

- `显示` / `隐藏` — mostrar u ocultar el cursor de la IA.
- `停止` / `启用` — 停止 baja el nivel de permiso a 0 y oculta el cursor; 启用 restaura el nivel anterior, como mínimo 2.
- `AI 提示` — la etiqueta bajo el cursor, con `开` / `关`.
- `透明度` — opacidad del cursor y de la etiqueta, del 15 % al 100 %.
- `颜色` — ocho muestras más el valor `#RRGGBB` actual; el color de acento tiñe el cursor, el borde de la etiqueta y los resaltados del panel.
- `造型` — `鲸鱼` (ballena, por defecto), `圆点` (punto) o `箭头` (flecha).
- `大小` — tamaño del cursor, del 60 % al 200 %.
- `权限等级` — `停止` (0), `只读` (1), `操作` (2), `管理员` (3).
- Pie — visible u oculto, coordenadas del cursor, fotogramas por segundo y nivel de permiso actual.

Notas:

- Toda la cabecera es la zona de arrastre, y el panel se queda donde lo sueltas.
- Plegar es solo con el ratón: haz clic en `─` o en `✕` de la cabecera y el panel se reduce al chip de 132×40 (`≡ AI 鼠标`); al hacer clic en el chip se despliega otra vez. La activación por teclado no está soportada a propósito. El equivalente por herramienta es `ai_mouse_panel { expanded: false }`, y `expanded: true` lo vuelve a abrir; no hay botón de ocultar: ocultar es `ai_mouse_panel { visible: false }`.
- Una sola función de maquetación produce los rectángulos dibujados y la lista de aciertos, así que el clic cae sobre lo que ves.
- El panel nunca toma el foco: tu teclado se queda en la aplicación en primer plano.

## Niveles de permiso

Los nombres son los que imprime el panel: 停止 (parado), 只读 (solo lectura), 操作 (conducir), 管理员 (administrador).

| Nivel | Nombre | Qué puede hacer la IA |
|---|---|---|
| 0 | 停止 | Nada salvo el estado y la fontanería de sistema de captura/inactividad: `ai_mouse_status` y `ai_mouse_capture_visible` siguen funcionando. |
| 1 | 只读 | Mostrar u ocultar el cursor y el panel. |
| 2 | 操作 | El nivel 1 más `ai_mouse_move`. |
| 3 | 管理员 | El nivel 2 más la apariencia (`label`, `theme`, `badge`, `opacity`, `color`, `scale`) y bajar el nivel. |

- Una persona puede poner cualquier nivel en el panel. El agente tiene una sola palanca, `ai_mouse_permission`, y solo va hacia abajo: un nivel por encima del actual se rechaza.
- Dos puertas independientes: la capa TypeScript comprueba el nivel mínimo de cada herramienta y el proceso Rust vuelve a comprobar `min_perm` para cada comando. Una caché obsoleta del lado del agente no puede escalar nada.
- 停止 significa nivel 0 más cursor oculto. 启用 restaura `max(nivel anterior, 2)`.
- `ai_mouse_stop` es una puerta de un solo sentido: oculta el cursor, baja el nivel a 0 y muestra el panel para que la persona pulse 启用.
- Un rechazo nombra la acción, el nivel que necesita, el nivel actual y dice que solo una persona puede subirlo; un agente que lo lee no tiene motivo para reintentar.

## Arquitectura

El shell del plugin registra las herramientas; la superposición es un pequeño programa Rust aparte que posee las dos ventanas. Hablan por stdin/stdout, un objeto JSON por línea.

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

- `src/index.ts` — plugin de Cordis: registra las catorce herramientas, filtra cada llamada y aplica el aspecto configurado en el primer uso.
- `src/overlay.ts` — dueño del proceso hijo y del protocolo de líneas: ids de petición, 3 s de espera por respuesta, 5 s de arranque, `quit` al descargar.
- `src/config.ts` — el esquema Schemastery; el host lo valida al cargar.
- `src/permissions.ts` — la tabla de niveles y las funciones puras, con pruebas unitarias sin ventana.
- `overlay/src/main.rs` — Win32: dos ventanas en capas, pintura GDI con supersampling, hit-test del panel, arrastre, deslizadores y muestras.

Las notas de diseño completas (en chino) están en [`docs/DESIGN.md`](docs/DESIGN.md). Cada respuesta repite el `id` de la petición y `status` devuelve el estado completo.

## Requisitos

| Elemento | Requisito |
|---|---|
| Sistema operativo | Windows 10 versión 2004 o posterior. La superposición necesita `WDA_EXCLUDEFROMCAPTURE`; las compilaciones más antiguas no pueden excluir una ventana de la captura. |
| Node.js | `^22.19.0 \|\| >=24.0.0` |
| pnpm | 11 o posterior, solo para compilar desde el código fuente |
| Rust | Una toolchain estable con el target de MSVC, solo para compilar la superposición desde el código fuente |
| Harness | Una compilación de DeepSeek Harness con el cargador de plugins Cordis; el rango admitido de `@deepseek-ai/dsh-tools` está declarado en `package.json` |

**Solo Windows.** macOS y Linux no están implementados: la superposición es un programa Win32 sin alternativa, así que en esas plataformas no hay nada con lo que dibujar un cursor.

## Instalación

Instala el paquete publicado, o enlaza un checkout local mientras desarrollas. Sustituye `desktop` por el nombre de tu perfil.

```sh
dsh plugin --profile desktop add dsh-ai-cursor
dsh plugin --profile desktop add link:/absolute/path/to/dsh-ai-cursor
```

Reinicia el harness después: los plugins se cargan al arrancar, así que uno nuevo solo aparece tras un reinicio. Los paquetes publicados incluyen `bin/ai-mouse-overlay.exe`; un checkout local tiene que compilarlo una vez con `pnpm run build:overlay`.

## Compilación

```sh
pnpm install
pnpm run build:overlay
pnpm run typecheck
pnpm test
pnpm run build
```

- `pnpm install` — dependencias de desarrollo de TypeScript (pnpm 11; el script de compilación de `esbuild` está permitido en `pnpm-workspace.yaml`).
- `pnpm run build:overlay` — `cargo build --release` en `overlay/` y luego copia `ai-mouse-overlay.exe` a `bin/`. Si `cargo` falla y ya hay un binario en `bin/`, se conserva ese binario.
- `pnpm run typecheck` — `tsc --noEmit`.
- `pnpm test` — `vitest run`.
- `pnpm run build` — `tsdown` empaqueta `lib/index.js` y `lib/index.d.ts`.

O pulsa `Ctrl+Shift+B` en VS Code y ejecuta la tarea `all (install → overlay → check → build)`, que encadena los mismos cinco pasos. Compilar la superposición requiere Rust estable con las herramientas de compilación de MSVC (`x86_64-pc-windows-msvc`); el shell del plugin solo necesita Node 22+.

## Configuración

Todas las claves están declaradas con Schemastery en `src/config.ts`; el host las valida al cargar y rechaza los valores inválidos.

| Clave | Valor por defecto | Significado |
|---|---|---|
| `enabled` | `true` | Registrar la familia de herramientas. |
| `exePath` | `""` | Ruta absoluta a `ai-mouse-overlay.exe`. Vacío resuelve el `bin/` incluido. |
| `label` | `"AI 鼠标"` | Texto de la etiqueta bajo el cursor. |
| `theme` | `"whale"` | Dibujo: `whale`, `dot` o `arrow`. |
| `idleMs` | `60000` | Ocultar el cursor tras esos ms sin moverse; `0` nunca oculta. |
| `captureVisible` | `false` | Permitir que el cursor aparezca en capturas y grabaciones. |
| `panel` | `true` | Mostrar el panel flotante la primera vez que se usa la superposición. |
| `panelExpanded` | `true` | Abrir como barra de menú completa; `false` empieza como chip. |
| `opacity` | `1` | Opacidad del cursor y la etiqueta, 0.15–1. |
| `color` | `"#4FC3F7"` | Acento `#RRGGBB` del cursor, el borde de la etiqueta y los resaltados del panel. |
| `scale` | `1` | Multiplicador de tamaño del cursor, 0.6–2. |
| `badge` | `true` | Mostrar la etiqueta `AI 提示`. |
| `permission` | `3` | Techo inicial: 0 停止, 1 只读, 2 操作, 3 管理员. |

`permission` es un punto de partida, no un candado: se aplica en la primera llamada a una herramienta y, a partir de ahí, manda el panel. Un agente puede bajarlo; solo una persona puede subirlo.

## Herramientas

Catorce herramientas. «Permiso mínimo» es el nivel más bajo en el que se permite la llamada; `0 · any` también funciona con el cursor parado.

| Herramienta | Permiso mínimo | Qué hace |
|---|---|---|
| `ai_mouse_show` | 1 · 只读 | Mostrar el cursor, aplicando el aspecto configurado la primera vez. |
| `ai_mouse_hide` | 1 · 只读 | Ocultar el cursor. |
| `ai_mouse_move` | 2 · 操作 | Deslizar a coordenadas absolutas en píxeles de dispositivo (`x`, `y`, `duration_ms?`; 320 por defecto, `0` salta). Muestra el cursor. |
| `ai_mouse_panel` | 1 · 只读 | Mostrar u ocultar, desplegar o plegar el panel (`visible?`, `expanded?`). |
| `ai_mouse_status` | 0 · any | Leer visibilidad, posición, etiqueta, dibujo, tamaño, opacidad, color, badge, permiso, estado del panel y fps. |
| `ai_mouse_capture_visible` | 0 · any | Permitir que el cursor aparezca en capturas (`enabled`); desactivado por defecto. |
| `ai_mouse_label` | 3 · 管理员 | Fijar el texto de la etiqueta (`text`); una cadena vacía oculta el badge. |
| `ai_mouse_theme` | 3 · 管理员 | Dibujo: `whale` (por defecto), `dot`, `arrow`. |
| `ai_mouse_badge` | 3 · 管理员 | Mostrar u ocultar la etiqueta `AI 提示` (`visible`). |
| `ai_mouse_opacity` | 3 · 管理员 | Opacidad del cursor y la etiqueta, 0.15–1. |
| `ai_mouse_color` | 3 · 管理员 | Color de acento como `#RRGGBB`. |
| `ai_mouse_scale` | 3 · 管理员 | Tamaño del cursor, 0.6–2. |
| `ai_mouse_permission` | 3 · 管理员 | Leer o bajar el nivel (`level?`, 0–3). Subirlo se rechaza. |
| `ai_mouse_stop` | 3 · 管理员 | Ocultar el cursor, bajar a nivel 0 y mostrar el panel. Solo una persona puede volver a arrancarlo. |

- `ai_mouse_permission` sin `level` solo lee, pero la llamada en sí necesita nivel 3; desde abajo, lee el nivel con `ai_mouse_status`.
- Cada herramienta devuelve una línea de texto; una llamada bloqueada devuelve la frase de rechazo en lugar de hacer algo.
- `ai_mouse_capture_visible` vuelve a leer el estado hasta que las ventanas recreadas informan del valor nuevo, así que su respuesta refleja el estado real.
- Flujo típico: `ai_mouse_move` hasta el objetivo, hacer clic con la herramienta de computer-use y luego `ai_mouse_hide`; o dejar el cursor visible y que lo oculte el tiempo de inactividad.

## Protocolo de línea

Un objeto JSON por línea en stdin; una respuesta por línea en stdout con el mismo `id`. Los comandos `system` son fontanería que el shell del plugin envía por sí mismo y no son accesibles desde una herramienta.

| Comando | Permiso requerido | Efecto |
|---|---|---|
| `{"cmd":"show"}` / `{"cmd":"hide"}` | ≥ 1 | Mostrar u ocultar el cursor. |
| `{"cmd":"move","x":900,"y":520,"duration_ms":320}` | ≥ 2 | Deslizar a coordenadas absolutas de pantalla. |
| `{"cmd":"panel","visible":true}` | ≥ 1 | Mostrar u ocultar el panel. |
| `{"cmd":"panel_expanded","expanded":true}` | ≥ 1 | Barra de menú completa, o el chip. |
| `{"cmd":"opacity","value":0.7}` | ≥ 3 | Opacidad del cursor y la etiqueta, 0.15–1. |
| `{"cmd":"color","hex":"#4FC3F7"}` | ≥ 3 | Color de acento. |
| `{"cmd":"scale","value":1.2}` | ≥ 3 | Tamaño del cursor, 0.6–2. |
| `{"cmd":"badge","visible":true}` | ≥ 3 | `AI 提示` activado o desactivado. |
| `{"cmd":"theme","theme":"whale"}` | ≥ 3 | `whale`, `dot` o `arrow`. |
| `{"cmd":"label","text":"AI 鼠标"}` | ≥ 3 | Texto de la etiqueta. |
| `{"cmd":"permission","level":2}` | ≥ 3, solo bajar | Nivel de permiso. |
| `{"cmd":"pause"}` / `{"cmd":"resume"}` | ≥ 3 | Igual que 停止 / 启用. |
| `{"cmd":"idle","idle_ms":60000}` | system | Ocultar tras ese tiempo de inactividad; `0` nunca. |
| `{"cmd":"affinity","capture_visible":false}` | system | Exclusión de captura; recrea las dos ventanas. |
| `{"cmd":"dump","path":"frame.bmp","target":"cursor"}` | system | Pintar un fotograma en un BMP (autoprueba, sin captura de pantalla). |
| `{"cmd":"status"}` | any | Leer el estado completo. |
| `{"cmd":"quit"}` | system | Salir del proceso de superposición. |

```json
{"id":"1","cmd":"move","x":900,"y":520,"duration_ms":320}
{"id":"2","cmd":"affinity","capture_visible":true}
{"id":"3","cmd":"theme","theme":"whale"}
{"id":"4","cmd":"dump","path":"frame.bmp","target":"cursor"}
{"cmd":"quit"}
```

Cada respuesta es `{"id":"<mismo id>","ok":true,…}`. Un comando rechazado responde `ok:false` y un `error` que indica el nivel que necesitaba. `status` añade `visible`, `x`, `y`, `label`, `theme`, `badge`, `opacity`, `scale`, `color`, `permission`, `panel`, `panel_expanded`, `hwnd`, `panel_hwnd`, `fps`, `capture_visible` y `excluded_from_capture`.

## Solución de problemas

### El cursor no se ve

- Comprueba el nivel: `ai_mouse_status` imprime `permission=`. En el nivel 0 (停止) el cursor está oculto a propósito; una persona tiene que pulsar 启用 o un nivel superior en el panel.
- El cursor se oculta solo tras `idleMs` sin moverse (60 s por defecto). Vuelve a llamar a `ai_mouse_show` o `ai_mouse_move`, o pon `idleMs: 0`.
- Si `ai_mouse_status` devuelve un error que menciona el ejecutable, mira el punto siguiente.
- Ocultar el panel no oculta el cursor, y ocultar el cursor no oculta el panel: son independientes.

### No se encuentra `ai-mouse-overlay.exe`

- Compílalo una vez: `pnpm run build:overlay` escribe `bin/ai-mouse-overlay.exe`.
- Sin `exePath`, la resolución prueba `<paquete>/bin/ai-mouse-overlay.exe` y luego la compilación de desarrollo en `overlay/target/release/ai-mouse-overlay.exe`.
- `exePath` debe ser una ruta absoluta; una ruta relativa se rechaza.
- El mensaje de error contiene la ruta absoluta que probó: léela antes de adivinar.

### El modo de captura no surtió efecto

- Windows permite `SetWindowDisplayAffinity` una sola vez por ventana: volver a `WDA_NONE` falla con `ERROR_NOT_ENOUGH_MEMORY (0x80070008)`. Por eso la superposición recrea las dos ventanas cada vez que cambia el modo de captura, y eso tiene que ocurrir en el hilo que posee el bucle de mensajes (crearlas en otro hilo bloquea `ShowWindow`).
- `ai_mouse_capture_visible` consulta el estado hasta que la superposición informa del valor pedido; si la respuesta aún muestra el valor antiguo, las ventanas todavía no se han recreado.
- Las compilaciones antiguas de Windows no tienen `WDA_EXCLUDEFROMCAPTURE`, así que allí la exclusión no hace nada en silencio.

### Permiso denegado

- El rechazo es deliberado: la herramienta necesita un nivel, la superposición está en uno más bajo y solo una persona puede subirlo.
- No reintentes con otros argumentos. La herramienta está filtrada y el comando se vuelve a filtrar dentro del proceso.
- Pide a la persona que pulse 管理员 en el panel, o 启用 si el nivel es 0.
- Si una persona acaba de subir el nivel, un reintento sí funciona: el plugin vuelve a leer el nivel cuando su valor en caché rechaza una llamada.

### Falta el panel

- `ai_mouse_panel { visible: true }` lo muestra; añade `expanded: true` para la barra de menú completa.
- El chip plegado mide 132×40 y se queda donde estaba el panel; las dos ventanas son topmost, así que está por encima de las ventanas normales.
- Con `panel: false` el panel simplemente no se muestra en el primer uso; una llamada a herramienta lo trae de vuelta.

### No existen las herramientas `ai_mouse_*`

- `enabled: false` no registra nada.
- Los plugins se cargan al arrancar: reinicia el harness después de instalar o actualizar.
- Asegúrate de que el perfil en el que instalaste es el perfil que estás ejecutando.

## Contribuir

Se agradecen issues, traducciones y parches. Antes de abrir un pull request, ejecuta `pnpm run typecheck` y `pnpm test`, y mantén intacta la invariante de permisos: un agente puede bajar su propio nivel, solo una persona lo sube.

Las cadenas en chino de `overlay/src/main.rs` y los nombres de nivel de `src/permissions.ts` son términos de producto — 显示 隐藏 停止 启用 只读 操作 管理员 AI 提示 造型 大小 透明度 颜色 权限等级 — no los renombres. Los cinco README se mantienen sincronizados: cambia la misma sección en todos.

## Licencia

Apache License 2.0 — consulta [LICENSE](LICENSE). © 2026 dsh-ai-cursor contributors.
