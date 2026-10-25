import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, describe, expect, it } from 'vitest'
import App from '../App.vue'

describe('App', () => {
  afterEach(() => clearMocks())

  it('调用 ping 后显示后端版本', async () => {
    mockIPC((cmd) => {
      if (cmd === 'ping') return { message: 'pong', appVersion: '0.1.0' }
    })
    const wrapper = mount(App)
    await flushPromises()
    expect(wrapper.get('[data-test="status"]').text()).toBe('后端已连接 · v0.1.0')
  })

  it('ping 失败时显示错误', async () => {
    mockIPC(() => {
      throw new Error('boom')
    })
    const wrapper = mount(App)
    await flushPromises()
    expect(wrapper.get('[data-test="status"]').text()).toContain('后端连接失败')
  })
})
