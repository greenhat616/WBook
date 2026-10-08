// Injected into the cover search window. It marks the image under the cursor
// with a button that hands the image to the app by navigating to a URL the
// app intercepts; the remote page has no access to the app's commands.
;(() => {
  if (window.top !== window || window.__wbookCoverPicker) return
  window.__wbookCoverPicker = true

  const PICK = 'https://wbook-cover.invalid/pick'
  const MIN_SIDE = 80

  // Result pages show thumbnails and keep the full-size address elsewhere.
  function source(image) {
    const bing = image.closest('[m]')
    if (bing) {
      try {
        const meta = JSON.parse(bing.getAttribute('m'))
        if (meta.murl) return meta.murl
      } catch {
        // Not Bing's metadata; fall back to the image itself.
      }
    }
    const baidu = image.closest('[data-objurl]')
    if (baidu) return baidu.getAttribute('data-objurl')
    const src = image.currentSrc || image.src
    // Douban serves the same cover in small, medium and large sizes.
    return /doubanio\.com\//.test(src)
      ? src.replace(/\/view\/subject\/[sm]\//, '/view/subject/l/')
      : src
  }

  const button = document.createElement('button')
  button.type = 'button'
  button.textContent = '设为封面'
  button.style.cssText =
    'position:fixed;z-index:2147483647;display:none;padding:6px 14px;border:0;border-radius:999px;background:#0b57d0;color:#fff;font:600 13px system-ui,sans-serif;box-shadow:0 2px 8px rgba(0,0,0,.35);cursor:pointer'
  let target = null

  button.addEventListener(
    'click',
    (event) => {
      event.preventDefault()
      event.stopPropagation()
      if (!target) return
      const url = new URL(PICK)
      url.searchParams.set('src', source(target))
      url.searchParams.set('page', location.href)
      location.href = url.href
    },
    true
  )

  // Overlays on result pages often cover the image, so look beneath them.
  let frame = 0
  document.addEventListener(
    'mousemove',
    (event) => {
      if (frame) return
      frame = requestAnimationFrame(() => {
        frame = 0
        const stack = document.elementsFromPoint(event.clientX, event.clientY)
        if (stack[0] === button) return
        const image = stack.find(
          (element) => element instanceof HTMLImageElement
        )
        const box = image?.getBoundingClientRect()
        if (!image || box.width < MIN_SIDE || box.height < MIN_SIDE) {
          button.style.display = 'none'
          target = null
          return
        }
        target = image
        button.style.left = `${Math.max(box.left, 0) + 8}px`
        button.style.top = `${Math.max(box.top, 0) + 8}px`
        button.style.display = 'block'
      })
    },
    true
  )
  window.addEventListener(
    'scroll',
    () => {
      button.style.display = 'none'
    },
    true
  )

  const attach = () => document.documentElement.appendChild(button)
  if (document.readyState === 'loading')
    document.addEventListener('DOMContentLoaded', attach)
  else attach()
})()
