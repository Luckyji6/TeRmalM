import { invoke } from '@tauri-apps/api/core'
import { enable, isEnabled } from '@tauri-apps/plugin-autostart'

export type TargetKind = 'local' | 'ssh'

export interface SshHost {
  alias: string
  hostname?: string | null
  user?: string | null
  port?: string | null
}

export interface Task {
  id: string
  name: string
  command: string
  cwd?: string | null
  target: TargetKind
  ssh_host?: string | null
  env_json: string
  auto_restart: boolean
  system_autostart: boolean
  created_at: number
  updated_at: number
}

export interface TaskInput {
  name: string
  command: string
  cwd?: string | null
  target: TargetKind
  ssh_host?: string | null
  env_json?: string
  auto_restart: boolean
  system_autostart: boolean
}

export interface RunningProcess {
  id: string
  running: boolean
}

export interface PtySnapshot {
  id: string
  output: string
  running: boolean
}

export interface ActionLog {
  id: string
  action: string
  entity_id?: string | null
  summary: string
  created_at: number
}

type TauriWindow = Window & {
  __TAURI_INTERNALS__?: unknown
}

const isTauri = typeof window !== 'undefined' && Boolean((window as TauriWindow).__TAURI_INTERNALS__)
const mockTasksKey = 'termalm.mock.tasks'

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri) {
    return mockInvoke<T>(command, args)
  }
  return invoke<T>(command, args)
}

export const api = {
  listSshHosts: () => call<SshHost[]>('list_ssh_hosts'),
  listTasks: () => call<Task[]>('list_tasks'),
  listUserActionLogs: (limit?: number) => call<ActionLog[]>('list_user_action_logs', { limit }),
  recordUserAction: (input: { action: string; entity_id?: string | null; summary: string }) =>
    call<ActionLog>('record_user_action', { input }),
  saveTask: (input: TaskInput) => call<Task>('save_task', { input }),
  deleteTask: (id: string) => call<void>('delete_task', { id }),
  startProcess: (input: {
    id: string
    command: string
    cwd?: string | null
    target: TargetKind
    ssh_host?: string | null
  }) => call<RunningProcess>('start_process', { input }),
  stopProcess: (id: string) => call<RunningProcess>('stop_process', { id }),
  processStatus: (id: string) => call<RunningProcess>('process_status', { id }),
  processLog: (id: string) => call<string>('process_log', { id }),
  verifySystemAuth: () => call<void>('verify_system_auth'),
  ptyStart: (input: {
    shell?: string | null
    ssh_host?: string | null
    cwd?: string | null
    cols?: number
    rows?: number
  }) => call<PtySnapshot>('pty_start', { input }),
  ptyWrite: (id: string, data: string) => call<void>('pty_write', { id, data }),
  ptyRead: (id: string) => call<PtySnapshot>('pty_read', { id }),
  ptyStop: (id: string) => call<void>('pty_stop', { id }),
  installAppAutostart: async () => {
    if (!isTauri) return 'Browser preview mode cannot install app autostart.'
    if (await isEnabled()) return 'App autostart is already enabled.'
    await enable()
    return 'App autostart enabled.'
  },
}

async function mockInvoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  await new Promise((resolve) => window.setTimeout(resolve, 120))
  if (command === 'list_ssh_hosts') {
    return [
      { alias: 'prod-web', hostname: 'prod.example.com', user: 'deploy', port: '22' },
      { alias: 'staging', hostname: 'staging.example.com', user: 'deploy', port: '22' },
    ] as T
  }
  if (command === 'list_tasks') {
    return readMockTasks() as T
  }
  if (command === 'list_user_action_logs') {
    return JSON.parse(localStorage.getItem('termalm.mock.actionLogs') ?? '[]') as T
  }
  if (command === 'record_user_action') {
    const input = args?.input as { action: string; entity_id?: string | null; summary: string }
    const log: ActionLog = {
      id: crypto.randomUUID(),
      action: input.action,
      entity_id: input.entity_id,
      summary: input.summary,
      created_at: Date.now(),
    }
    const current = JSON.parse(localStorage.getItem('termalm.mock.actionLogs') ?? '[]') as ActionLog[]
    localStorage.setItem('termalm.mock.actionLogs', JSON.stringify([log, ...current].slice(0, 500)))
    return log as T
  }
  if (command === 'save_task') {
    const input = (args?.input ?? {}) as TaskInput
    const now = Date.now()
    const task: Task = {
      id: crypto.randomUUID(),
      name: input.name,
      command: input.command,
      cwd: input.cwd,
      target: input.target,
      ssh_host: input.ssh_host,
      env_json: input.env_json ?? '{}',
      auto_restart: input.auto_restart,
      system_autostart: input.system_autostart,
      created_at: now,
      updated_at: now,
    }
    writeMockTasks([task, ...readMockTasks()])
    return task as T
  }
  if (command === 'delete_task') {
    const id = args?.id as string
    writeMockTasks(readMockTasks().filter((task) => task.id !== id))
    return undefined as T
  }
  if (command === 'process_log') {
    return 'Browser preview mode: run npm run tauri:dev to execute real processes.\necho output appears here in the desktop app.\n' as T
  }
  if (command === 'verify_system_auth') {
    return undefined as T
  }
  if (command === 'pty_start') {
    return {
      id: crypto.randomUUID(),
      output: 'TeRmalM browser preview terminal\nRun inside Tauri for a real PTY session.\n\n',
      running: true,
    } as T
  }
  if (command === 'pty_read') {
    return { id: String(args?.id ?? ''), output: '', running: true } as T
  }
  return { id: String(args?.id ?? ''), running: command !== 'stop_process' } as T
}

function readMockTasks(): Task[] {
  try {
    return JSON.parse(localStorage.getItem(mockTasksKey) ?? '[]') as Task[]
  } catch {
    return []
  }
}

function writeMockTasks(tasks: Task[]) {
  localStorage.setItem(mockTasksKey, JSON.stringify(tasks))
}
