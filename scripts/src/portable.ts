import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { parseArgs } from 'node:util'
import { zipSync } from 'fflate'

const { values } = parseArgs({
  options: { target: { type: 'string' } }
})
const target = values.target
if (!target?.endsWith('-pc-windows-msvc')) {
  throw new Error('--target must be a Windows MSVC target triple')
}

const root = resolve(import.meta.dirname, '../..')
const config = JSON.parse(
  readFileSync(join(root, 'backend/tauri/tauri.conf.json'), 'utf8')
)
const binary = `${config.mainBinaryName}.exe`
const release = join(root, 'backend/target', target, 'release')
// Same architecture names as the installers Tauri bundles next to it.
const arch = target.startsWith('aarch64') ? 'arm64' : 'x64'

const name = `${config.productName}_${config.version}_${arch}_portable`

// A top-level folder keeps the files together however the archive is extracted.
const zip = zipSync({
  [name]: {
    [binary]: readFileSync(join(release, binary)),
    // Tells the app to keep its data next to the executable.
    '.portable': new Uint8Array()
  }
})
const outDir = join(release, 'bundle/portable')
mkdirSync(outDir, { recursive: true })
const out = join(outDir, `${name}.zip`)
writeFileSync(out, zip)
console.log(out)
