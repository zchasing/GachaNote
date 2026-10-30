// 分享图（UI/UX 文档 3.7，F-20）：用 Canvas 2D 按固定版式绘制，导出 1080 px 宽的 PNG。
// 版式按 540 px 设计、以 2 倍绘制；配色取自设计 token 的浅色 / 深色两套值，与界面当前主题无关。

export interface SharePalette {
  bg: string
  surface: string
  border: string
  text: string
  text2: string
  gold: string
  up: string
  upbg: string
  loss: string
  lossbg: string
  neutral: string
  track: string
}

/** 与 theme/tokens.css 一致。 */
export const PALETTES: Record<'light' | 'dark', SharePalette> = {
  light: {
    bg: '#f7f7f8',
    surface: '#ffffff',
    border: '#e4e4e7',
    text: '#18181b',
    text2: '#5f5f68',
    gold: '#9a6700',
    up: '#1f7a4d',
    upbg: '#e3f4ea',
    loss: '#c23434',
    lossbg: '#fbe7e7',
    neutral: '#ededf0',
    track: '#ededf0',
  },
  dark: {
    bg: '#141518',
    surface: '#1d1f23',
    border: '#2e3036',
    text: '#ececee',
    text2: '#a0a0aa',
    gold: '#e3a43b',
    up: '#5ccb94',
    upbg: '#183528',
    loss: '#f07a7a',
    lossbg: '#3a1e1e',
    neutral: '#2a2c31',
    track: '#2a2c31',
  },
}

export interface ShareMetric {
  label: string
  value: string
  sub: string
}

export interface ShareRow {
  name: string
  pulls: string
  tag: { text: string; kind: 'up' | 'loss' | 'neutral' } | null
}

export interface ShareContent {
  /** 游戏图标的字标与主题色。 */
  mono: string
  color: string
  ink: string
  title: string
  subtitle: string
  metrics: ShareMetric[]
  listTitle: string
  rows: ShareRow[]
  empty: string
  date: string
  app: string
}

/** 按主题色亮度选深色或白色字，保证对比度（游戏字标用）。 */
export function inkOn(color: string): string {
  const hex = color.replace('#', '')
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255)
  const lum = 0.2126 * r! + 0.7152 * g! + 0.0722 * b!
  return lum > 0.5 ? '#1A1A12' : '#FFFFFF'
}

/** 导出宽度（px）与绘制倍数。 */
export const SHARE_WIDTH = 1080
const SCALE = 2
const W = SHARE_WIDTH / SCALE
const PAD = 28
const ROW_H = 30

/** 按内容计算高度（设计单位）。 */
function layoutHeight(c: ShareContent): number {
  const rows = Math.max(1, c.rows.length)
  return PAD + 44 + 20 + 96 + 24 + 20 + 8 + rows * ROW_H + 16 + 40 + PAD - 10
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath()
  ctx.moveTo(x + r, y)
  ctx.arcTo(x + w, y, x + w, y + h, r)
  ctx.arcTo(x + w, y + h, x, y + h, r)
  ctx.arcTo(x, y + h, x, y, r)
  ctx.arcTo(x, y, x + w, y, r)
  ctx.closePath()
}

function ellipsis(ctx: CanvasRenderingContext2D, text: string, max: number): string {
  if (ctx.measureText(text).width <= max) return text
  let s = text
  while (s.length > 1 && ctx.measureText(`${s}…`).width > max) s = s.slice(0, -1)
  return `${s}…`
}

/** 在 `canvas` 上绘制分享图，并把画布调整为导出尺寸。拿不到 2D 上下文时（如测试环境）只调整尺寸。 */
export function drawShare(canvas: HTMLCanvasElement, c: ShareContent, p: SharePalette, font: string) {
  const H = layoutHeight(c)
  canvas.width = SHARE_WIDTH
  canvas.height = H * SCALE
  const ctx = canvas.getContext('2d')
  if (!ctx) return
  ctx.setTransform(SCALE, 0, 0, SCALE, 0, 0)
  ctx.textBaseline = 'middle'
  const f = (size: number, weight = 400) => `${weight} ${size}px ${font}`

  // 背景与卡片
  ctx.fillStyle = p.bg
  ctx.fillRect(0, 0, W, H)
  roundRect(ctx, 12, 12, W - 24, H - 24, 14)
  ctx.fillStyle = p.surface
  ctx.fill()
  ctx.strokeStyle = p.border
  ctx.lineWidth = 1
  ctx.stroke()
  // 顶部游戏色条
  ctx.save()
  roundRect(ctx, 12, 12, W - 24, H - 24, 14)
  ctx.clip()
  ctx.fillStyle = c.color
  ctx.fillRect(12, 12, W - 24, 4)
  ctx.restore()

  let y = PAD + 6
  // 标题：字标 + 游戏 · 备注名，下一行 UID · 卡池
  roundRect(ctx, PAD, y, 40, 40, 10)
  ctx.fillStyle = c.color
  ctx.fill()
  ctx.fillStyle = c.ink
  ctx.font = f(20, 700)
  ctx.textAlign = 'center'
  ctx.fillText(c.mono, PAD + 20, y + 21)
  ctx.textAlign = 'left'
  ctx.fillStyle = p.text
  ctx.font = f(17, 600)
  ctx.fillText(ellipsis(ctx, c.title, W - PAD * 2 - 56), PAD + 52, y + 11)
  ctx.fillStyle = p.text2
  ctx.font = f(12)
  ctx.fillText(ellipsis(ctx, c.subtitle, W - PAD * 2 - 56), PAD + 52, y + 31)
  y += 44 + 20

  // 三项核心指标
  const gap = 10
  const mw = (W - PAD * 2 - gap * 2) / 3
  c.metrics.slice(0, 3).forEach((m, i) => {
    const x = PAD + i * (mw + gap)
    roundRect(ctx, x, y, mw, 96, 10)
    ctx.fillStyle = p.bg
    ctx.fill()
    ctx.strokeStyle = p.border
    ctx.stroke()
    ctx.fillStyle = p.text2
    ctx.font = f(12)
    ctx.fillText(ellipsis(ctx, m.label, mw - 24), x + 12, y + 20)
    ctx.fillStyle = p.text
    ctx.font = f(26, 700)
    ctx.fillText(ellipsis(ctx, m.value, mw - 24), x + 12, y + 52)
    ctx.fillStyle = p.text2
    ctx.font = f(11)
    ctx.fillText(ellipsis(ctx, m.sub, mw - 24), x + 12, y + 80)
  })
  y += 96 + 24

  // 最近出金
  ctx.fillStyle = p.text2
  ctx.font = f(12)
  ctx.fillText(c.listTitle, PAD, y + 10)
  y += 20 + 8
  if (c.rows.length === 0) {
    ctx.fillStyle = p.text2
    ctx.font = f(13)
    ctx.fillText(c.empty, PAD, y + ROW_H / 2)
  }
  c.rows.forEach((r, i) => {
    const ry = y + i * ROW_H
    if (i > 0) {
      ctx.strokeStyle = p.track
      ctx.beginPath()
      ctx.moveTo(PAD, ry)
      ctx.lineTo(W - PAD, ry)
      ctx.stroke()
    }
    const mid = ry + ROW_H / 2
    let right = W - PAD
    if (r.tag) {
      ctx.font = f(11, 600)
      const tw = ctx.measureText(r.tag.text).width + 12
      right -= tw
      roundRect(ctx, right, mid - 9, tw, 18, 9)
      ctx.fillStyle = r.tag.kind === 'up' ? p.upbg : r.tag.kind === 'loss' ? p.lossbg : p.neutral
      ctx.fill()
      ctx.fillStyle = r.tag.kind === 'up' ? p.up : r.tag.kind === 'loss' ? p.loss : p.text2
      ctx.textAlign = 'center'
      ctx.fillText(r.tag.text, right + tw / 2, mid + 1)
      ctx.textAlign = 'left'
      right -= 10
    }
    ctx.font = f(13)
    ctx.fillStyle = p.text
    ctx.textAlign = 'right'
    ctx.fillText(r.pulls, right, mid)
    const pw = ctx.measureText(r.pulls).width
    ctx.textAlign = 'left'
    ctx.fillStyle = p.gold
    ctx.font = f(14, 600)
    ctx.fillText(ellipsis(ctx, r.name, right - pw - PAD - 16), PAD, mid)
  })
  y += Math.max(1, c.rows.length) * ROW_H + 16

  // 页脚：日期与应用名
  ctx.strokeStyle = p.border
  ctx.beginPath()
  ctx.moveTo(PAD, y)
  ctx.lineTo(W - PAD, y)
  ctx.stroke()
  ctx.fillStyle = p.text2
  ctx.font = f(12)
  ctx.fillText(c.date, PAD, y + 20)
  ctx.textAlign = 'right'
  ctx.font = f(12, 600)
  ctx.fillText(c.app, W - PAD, y + 20)
  ctx.textAlign = 'left'
}

/** 画布内容的 base64（不含 `data:` 前缀）。 */
export function canvasPngBase64(canvas: HTMLCanvasElement): string {
  const url = canvas.toDataURL('image/png')
  return url.slice(url.indexOf(',') + 1)
}
