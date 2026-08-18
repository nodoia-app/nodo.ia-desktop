# Nodo Desktop (Tauri)

Misma web en ventana propia. Sin rediseño.
Instalador liviano — usa el WebView del sistema.

## URLs (importante)

| Modo | Qué carga |
|------|-----------|
| `npm run dev` | **`http://localhost:3000/agent`** — tu Next local + `.env.local` (staging backend) |
| `npm run build` (instalador) | **`https://login.nodoia.app`** — production |
| Override | `NODO_APP_URL=https://tu-staging…/agent npm run dev` |

La ventana carga el producto (`login.nodoia.app`). Links a `docs.nodoia.app` (términos, etc.) se abren en el navegador del sistema.

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

Tauri **no cruza de Mac a Windows**. Este Mac saca el `.dmg`. El `.exe` de los locales sale en GitHub Actions (runner `windows-latest`).

### Windows `.exe` (el que importa)

1. Push a `main` (incluye impresora LAN).
2. GitHub → **Actions** → **Build installers** → **Run workflow**.
3. Baja el artifact `Nodo-windows-x64` → `Nodo_1.0.0_x64-setup.exe`.

El instalador es NSIS, current-user (no pide admin) y trae WebView2 si el PC no lo tiene.

### Mac `.dmg` (local)

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
