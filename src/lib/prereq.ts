import { invoke } from '@tauri-apps/api/core'
import { trace } from './trace'

export interface PrereqStatus {
  missing: string[]
}

const MESSAGE_KEYS: Record<string, string> = {
  'webkit2gtk-4.1': 'linux.prereq.missingWebkit',
  libfuse2: 'linux.prereq.missingFuse',
}

export const prereqMessages = (
  missing: string[],
  t: (key: string) => string,
): string[] =>
  missing.flatMap((id) => {
    const key = MESSAGE_KEYS[id]
    return key ? [t(key)] : []
  })

// Non-blocking by design: a failed or slow probe never stops the flow.
export const logPrereqWarnings = async (
  t: (key: string) => string,
): Promise<void> => {
  try {
    const status = await invoke<PrereqStatus>('linux_prereq_status')
    prereqMessages(status.missing, t).forEach((message) => trace.log('flow', message))
  } catch {
    // The check is advisory, so an IPC failure stays silent.
  }
}
