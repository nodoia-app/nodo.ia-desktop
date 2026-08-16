# Nodo Desktop (Tauri)

Misma web en ventana propia. Sin rediseño.
Instalador liviano — usa el WebView del sistema.

## URLs (importante)

| Modo | Qué carga |
|------|-----------|
| `npm run dev` | **`http://localhost:3000/agent`** — tu Next local + `.env.local` (staging backend) |
| `npm run build` (instalador) | **`https://www.nodoia.app/agent`** — production |
| Override | `NODO_APP_URL=https://tu-staging…/agent npm run dev` |

No uses production mientras desarrollas. Arranca `nodo.ia-web` en `:3000` y luego el desktop.

## Requisitos (solo para build)

- Node.js
- Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- macOS: Xcode CLT
- Windows: VS Build Tools + WebView2

## Dev (staging vía local)

```bash
# terminal 1 — web con backend staging (.env.local)
cd ../nodo.ia-web && npm run dev

# terminal 2 — shell Tauri
cd ../nodo.ia-desktop
npm install
npm run dev
```

Staging frontend desplegado (si tienes URL):

```bash
NODO_APP_URL=https://TU-STAGING.vercel.app/agent npm run dev
```

## Build instalador (apunta a production)

```bash
npm run build
```

Salida: `src-tauri/target/release/bundle/`

Para un instalador de **staging** (QA interno):

```bash
NODO_APP_URL=https://TU-STAGING…/agent npm run build
```

## Publicar descarga

En `nodo.ia-web`:

```bash
NEXT_PUBLIC_DESKTOP_DOWNLOAD_URL=https://…/Nodo-setup.exe
NEXT_PUBLIC_DESKTOP_DOWNLOAD_URL_MAC=https://…/Nodo.dmg
```
