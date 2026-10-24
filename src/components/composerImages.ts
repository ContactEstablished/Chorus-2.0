import { commands, unwrap } from '../ipc'

/** A paste may expose files, items, or both. Never attach the same clipboard entry twice. */
export function clipboardImages(data: DataTransfer | null): File[] {
  if (!data) return []
  const files = Array.from(data.files)
  const candidates = files.length ? files : Array.from(data.items)
    .filter(item => item.kind === 'file')
    .map(item => item.getAsFile()).filter((file): file is File => !!file)
  return candidates.filter(file => file.type.startsWith('image/') ||
    ((!file.type || file.type === 'application/octet-stream') && /\.(png|jpe?g|webp)$/i.test(file.name)))
}

export function dataUrl(bytes: number[]): string {
  let binary = ''
  for (let i = 0; i < bytes.length; i += 8192) binary += String.fromCharCode(...bytes.slice(i, i + 8192))
  return `data:image/png;base64,${btoa(binary)}`
}
export function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image()
    image.onload = () => resolve(image)
    image.onerror = () => reject(new Error('This image could not be decoded. Choose a PNG, JPEG or WebP image.'))
    image.src = src
  })
}
export async function imagePreview(session: number, id: number, thumbnail = false): Promise<string> {
  const src = dataUrl(await unwrap(commands.sessionImageRead(session, id)))
  if (!thumbnail) return src
  const image = await loadImage(src)
  const scale = Math.min(1, 240 / Math.max(image.naturalWidth, image.naturalHeight))
  const canvas = document.createElement('canvas')
  canvas.width = Math.max(1, Math.round(image.naturalWidth * scale))
  canvas.height = Math.max(1, Math.round(image.naturalHeight * scale))
  canvas.getContext('2d')!.drawImage(image, 0, 0, canvas.width, canvas.height)
  return canvas.toDataURL('image/png')
}

/** Store pixels, not original metadata; the same normalized PNG is previewed and delivered. */
export async function prepareImage(file: File): Promise<{ name: string; png: number[] }> {
  // Windows file clipboard entries can omit the MIME type. Decoding below still validates pixels.
  if (!file.type || file.type === 'application/octet-stream') {
    const extension = file.name.split('.').pop()?.toLowerCase()
    const type = extension && ({ png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', webp: 'image/webp' } as Record<string, string>)[extension]
    if (type) file = new File([file], file.name, { type })
  }
  if (!['image/png', 'image/jpeg', 'image/webp'].includes(file.type)) throw new Error('Choose a PNG, JPEG or WebP image.')
  if (file.size > 20 * 1024 * 1024) throw new Error('Choose an image smaller than 20 MiB.')
  const src = await new Promise<string>((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(String(reader.result))
    reader.onerror = () => reject(new Error('Could not read the selected image.'))
    reader.readAsDataURL(file)
  })
  const image = await loadImage(src)
  const scale = Math.min(1, 4096 / Math.max(image.naturalWidth, image.naturalHeight))
  const canvas = document.createElement('canvas')
  canvas.width = Math.max(1, Math.round(image.naturalWidth * scale))
  canvas.height = Math.max(1, Math.round(image.naturalHeight * scale))
  canvas.getContext('2d')!.drawImage(image, 0, 0, canvas.width, canvas.height)
  const blob = await new Promise<Blob | null>(r => canvas.toBlob(r, 'image/png'))
  if (!blob || blob.size > 5 * 1024 * 1024) throw new Error('This image is too large after conversion. Use a smaller image (maximum 5 MiB per attachment).')
  return { name: file.name || 'Pasted image.png', png: Array.from(new Uint8Array(await blob.arrayBuffer())) }
}
