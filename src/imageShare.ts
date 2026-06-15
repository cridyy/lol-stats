import { toBlob } from "html-to-image"
import { copyPngToClipboard } from "./api"

type CopyElementOptions = {
  backgroundColor?: string
  filter?: (node: Node) => boolean
  pixelRatio?: number
}

export async function copyElementAsPng(
  element: HTMLElement,
  options: CopyElementOptions = {},
) {
  await document.fonts?.ready
  const blob = await toBlob(element, {
    backgroundColor: options.backgroundColor ?? "#f6faf9",
    cacheBust: true,
    pixelRatio: options.pixelRatio ?? 2,
    filter: options.filter,
  })

  if (!blob) throw new Error("图片生成失败")

  await writeImageBlobToClipboard(blob)
}

async function writeImageBlobToClipboard(blob: Blob) {
  const bytes = Array.from(new Uint8Array(await blob.arrayBuffer()))
  try {
    await copyPngToClipboard(bytes)
    return
  } catch (backendError) {
    if (!navigator.clipboard || typeof ClipboardItem === "undefined") {
      throw backendError
    }

    try {
      await navigator.clipboard.write([
        new ClipboardItem({
          [blob.type || "image/png"]: blob,
        }),
      ])
    } catch {
      throw backendError
    }
  }
}
