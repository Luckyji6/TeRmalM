import { useEffect, useMemo, useRef, useState } from 'react'
import { FitAddon } from '@xterm/addon-fit'
import { Terminal } from '@xterm/xterm'
import {
  Activity,
  Bot,
  Cable,
  CircleStop,
  ListRestart,
  Play,
  Plus,
  Power,
  RefreshCcw,
  Server,
  SquareTerminal,
  Trash2,
  X,
} from 'lucide-react'
import '@xterm/xterm/css/xterm.css'
import './App.css'
import { api, type SshHost, type TargetKind, type Task } from './lib/api'

interface RunningState {
  [taskId: string]: boolean
}

interface TerminalTab {
  key: string
  backendId?: string
  title: string
  sshHost?: string
  running: boolean
}

const initialForm = {
  name: 'Local dev server',
  command: 'npm run dev',
  cwd: '',
  target: 'local' as TargetKind,
  sshHost: '',
  autoRestart: false,
  systemAutostart: false,
}

function App() {
  const [tasks, setTasks] = useState<Task[]>([])
  const [hosts, setHosts] = useState<SshHost[]>([])
  const [running, setRunning] = useState<RunningState>({})
  const [selectedTaskId, setSelectedTaskId] = useState<string>()
  const [selectedLog, setSelectedLog] = useState('')
  const logRef = useRef<HTMLPreElement | null>(null)
  const [form, setForm] = useState(initialForm)
  const [status, setStatus] = useState('Ready')
  const [tabs, setTabs] = useState<TerminalTab[]>([])
  const [activeTab, setActiveTab] = useState<string>()
  const [isTaskModalOpen, setIsTaskModalOpen] = useState(false)
  const [isTerminalOpen, setIsTerminalOpen] = useState(false)
  const [revealedCommandId, setRevealedCommandId] = useState<string>()
  const [updateAvailable, setUpdateAvailable] = useState<string>()

  const selectedTask = useMemo(
    () => tasks.find((task) => task.id === selectedTaskId),
    [selectedTaskId, tasks],
  )

  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight
    }
  }, [selectedLog])

  useEffect(() => {
    void refresh()
    void checkForUpdate()
  }, [])

  useEffect(() => {
    const timer = window.setInterval(() => {
      void pollRunningTasks()
    }, 1800)
    return () => window.clearInterval(timer)
  })

  async function checkForUpdate() {
    try {
      const response = await fetch('https://api.github.com/repos/Luckyji6/TeRmalM/releases/latest')
      if (!response.ok) return
      const data = await response.json() as { tag_name: string }
      const latest = data.tag_name.replace(/^v/, '')
      const current = __APP_VERSION__.replace(/^v/, '')
      if (latest !== current) {
        setUpdateAvailable(data.tag_name)
      }
    } catch {
      // silently ignore — no network or rate limit
    }
  }

  async function refresh() {
    try {
      const [nextTasks, nextHosts] = await Promise.all([api.listTasks(), api.listSshHosts()])
      setTasks(nextTasks)
      setHosts(nextHosts)
      setStatus(`Loaded ${nextTasks.length} tasks and ${nextHosts.length} SSH hosts`)
    } catch (error) {
      setStatus(errorMessage(error))
    }
  }

  async function pollRunningTasks() {
    const ids = Object.entries(running)
      .filter(([, isRunning]) => isRunning)
      .map(([id]) => id)
    if (ids.length === 0) return
    const updates = await Promise.all(ids.map((id) => api.processStatus(id).catch(() => ({ id, running: false }))))
    setRunning((current) => {
      const next = { ...current }
      for (const item of updates) next[item.id] = item.running
      return next
    })
    if (selectedTask && ids.includes(selectedTask.id)) {
      setSelectedLog(await api.processLog(selectedTask.id))
    }
  }

  async function saveTask() {
    try {
      const task = await api.saveTask({
        name: form.name,
        command: form.command,
        cwd: form.cwd || null,
        target: form.target,
        ssh_host: form.target === 'ssh' ? form.sshHost : null,
        env_json: '{}',
        auto_restart: form.autoRestart,
        system_autostart: form.target === 'local' && form.systemAutostart,
      })
      setTasks((current) => [task, ...current])
      setSelectedTaskId(task.id)
      setIsTaskModalOpen(false)
      setForm(initialForm)
      setStatus(`Saved ${task.name}`)
      void logAction('task.create', task.id, `Created task ${task.name}`)
    } catch (error) {
      setStatus(errorMessage(error))
    }
  }

  async function startTask(task: Task) {
    try {
      const result = await api.startProcess({
        id: task.id,
        command: task.command,
        cwd: task.cwd,
        target: task.target,
        ssh_host: task.ssh_host,
      })
      setRunning((current) => ({ ...current, [task.id]: result.running }))
      setSelectedTaskId(task.id)
      setSelectedLog(await api.processLog(task.id))
      setStatus(`Started ${task.name}`)
      void logAction('task.start', task.id, `Started task ${task.name}`)
      window.setTimeout(() => {
        void api.processLog(task.id).then(setSelectedLog).catch(() => undefined)
      }, 350)
    } catch (error) {
      setStatus(errorMessage(error))
    }
  }

  async function stopTask(task: Task) {
    try {
      const result = await api.stopProcess(task.id)
      setRunning((current) => ({ ...current, [task.id]: result.running }))
      setSelectedLog(await api.processLog(task.id))
      setStatus(`Stopped ${task.name}`)
      void logAction('task.stop', task.id, `Stopped task ${task.name}`)
    } catch (error) {
      setStatus(errorMessage(error))
    }
  }

  async function removeTask(task: Task) {
    try {
      await api.deleteTask(task.id)
      setTasks((current) => current.filter((item) => item.id !== task.id))
      setRunning((current) => {
        const next = { ...current }
        delete next[task.id]
        return next
      })
      setStatus(`Deleted ${task.name}`)
      void logAction('task.delete', task.id, `Deleted task ${task.name}`)
    } catch (error) {
      setStatus(errorMessage(error))
    }
  }

  async function openTerminal(sshHost?: string) {
    setIsTerminalOpen(true)
    const key = crypto.randomUUID()
    const title = sshHost ? `ssh ${sshHost}` : 'local shell'
    void logAction('terminal.open', null, `Opened ${title}`)
    const optimisticTab: TerminalTab = { key, title, sshHost, running: true }
    setTabs((current) => [...current, optimisticTab])
    setActiveTab(key)
    try {
      const session = await api.ptyStart({ ssh_host: sshHost || null, cols: 110, rows: 30 })
      setTabs((current) =>
        current.map((tab) => (tab.key === key ? { ...tab, backendId: session.id } : tab)),
      )
    } catch (error) {
      setTabs((current) =>
        current.map((tab) => (tab.key === key ? { ...tab, running: false } : tab)),
      )
      setStatus(errorMessage(error))
    }
  }

  async function closeTerminal(tab: TerminalTab) {
    if (tab.backendId) {
      await api.ptyStop(tab.backendId).catch(() => undefined)
    }
    void logAction('terminal.close', tab.backendId ?? tab.key, `Closed ${tab.title}`)
    setTabs((current) => {
      const remaining = current.filter((item) => item.key !== tab.key)
      setActiveTab((currentActive) => {
        if (currentActive !== tab.key) return currentActive
        return remaining.at(-1)?.key
      })
      if (remaining.length === 0) {
        setIsTerminalOpen(false)
      }
      return remaining
    })
  }

  async function revealCommand(task: Task) {
    try {
      setStatus('Waiting for system verification...')
      await api.verifySystemAuth()
      setRevealedCommandId(task.id)
      setStatus('Full command revealed for 30 seconds')
      void logAction('command.reveal', task.id, `Revealed command for ${task.name}`)
      window.setTimeout(() => {
        setRevealedCommandId((current) => (current === task.id ? undefined : current))
      }, 30_000)
    } catch (error) {
      setStatus(errorMessage(error))
    }
  }

  async function logAction(action: string, entityId: string | null, summary: string) {
    await api.recordUserAction({ action, entity_id: entityId, summary }).catch(() => undefined)
  }

  return (
    <main className="app-shell">
      <aside className="sidebar">
        <header className="brand">
          <SquareTerminal size={24} />
          <div>
            <h1>TeRmalM</h1>
            <p>Command process manager</p>
          </div>
        </header>

        <button type="button" className="primary sidebar-action" onClick={() => setIsTaskModalOpen(true)}>
          <Plus size={16} />
          New task
        </button>

        <section className="host-list">
          <div className="panel-title">
            <Server size={16} />
            <span>SSH hosts</span>
          </div>
          {hosts.length === 0 ? (
            <p className="muted">No hosts found in ~/.ssh/config</p>
          ) : (
            hosts.map((host) => (
              <button key={host.alias} className="host-row" type="button" onClick={() => openTerminal(host.alias)}>
                <Cable size={15} />
                <span>{host.alias}</span>
                <small>{host.user ? `${host.user}@` : ''}{host.hostname ?? ''}</small>
              </button>
            ))
          )}
        </section>
      </aside>

      <section className="workspace">
        <nav className="toolbar">
          <button type="button" onClick={() => setIsTaskModalOpen(true)}>
            <Plus size={16} />
            New task
          </button>
          <button type="button" onClick={() => void refresh()}>
            <RefreshCcw size={16} />
            Refresh
          </button>
          <button type="button" onClick={() => void openTerminal()}>
            <SquareTerminal size={16} />
            Local terminal
          </button>
          <button type="button" onClick={() => void api.installAppAutostart().then(setStatus).catch((error) => setStatus(errorMessage(error)))}>
            <Power size={16} />
            App autostart
          </button>
          {updateAvailable && (
            <a
              className="update-badge"
              href={`https://github.com/Luckyji6/TeRmalM/releases/tag/${updateAvailable}`}
              target="_blank"
              rel="noreferrer"
            >
              Update available: {updateAvailable}
            </a>
          )}
          <span>{status}</span>
        </nav>

        <div className={`main-grid ${selectedTask ? 'has-details' : ''}`}>
          <section className="task-panel">
            <div className="section-heading">
              <Activity size={17} />
              <h2>Tasks</h2>
            </div>
            <div className="task-list">
              {tasks.map((task) => (
                <article
                  key={task.id}
                  className={`task-row ${selectedTask?.id === task.id ? 'selected' : ''}`}
                  onClick={() => setSelectedTaskId(task.id)}
                >
                  <div>
                    <strong>{task.name}</strong>
                    <code>{maskCommandPreview(task.command)}</code>
                  </div>
                  <div className="task-meta">
                    <span className={running[task.id] ? 'badge running' : 'badge'}>{running[task.id] ? 'running' : task.target}</span>
                    {task.system_autostart && <span className="badge">autostart</span>}
                  </div>
                  <div className="row-actions">
                    <button type="button" title="Start" onClick={(event) => { event.stopPropagation(); void startTask(task) }}>
                      <Play size={15} />
                    </button>
                    <button type="button" title="Stop" onClick={(event) => { event.stopPropagation(); void stopTask(task) }}>
                      <CircleStop size={15} />
                    </button>
                    <button type="button" title="Delete" onClick={(event) => { event.stopPropagation(); void removeTask(task) }}>
                      <Trash2 size={15} />
                    </button>
                  </div>
                </article>
              ))}
              {tasks.length === 0 && <p className="empty">Create a command task to start managing processes.</p>}
            </div>
          </section>

          {selectedTask && (
            <section className="details-panel">
              <header className="details-header">
                <div>
                  <div className="section-heading">
                    <ListRestart size={17} />
                    <h2>Details</h2>
                  </div>
                  <p>{selectedTask.name}</p>
                </div>
                <span className={running[selectedTask.id] ? 'badge running' : 'badge'}>
                  {running[selectedTask.id] ? 'running' : selectedTask.target}
                </span>
              </header>
              <div className="details-summary">
                <div className="detail-item">
                  <span className="detail-label">Target</span>
                  <span className="detail-value">{selectedTask.target === 'ssh' ? `SSH ${selectedTask.ssh_host}` : 'Local machine'}</span>
                </div>
                <div className="detail-item">
                  <span className="detail-label">Working directory</span>
                  <span className="detail-value">{selectedTask.cwd || 'Default shell directory'}</span>
                </div>
                <div className="detail-item">
                  <span className="detail-label">Restart</span>
                  <span className="detail-value">{selectedTask.auto_restart ? 'Enabled' : 'Disabled'}</span>
                </div>
                <div className="detail-item">
                  <span className="detail-label">System autostart</span>
                  <span className="detail-value">{selectedTask.system_autostart ? 'Enabled' : 'Disabled'}</span>
                </div>
                <div className="detail-item command-preview">
                  <span className="detail-label">Command preview</span>
                  <div className="command-row">
                    <code>
                      {revealedCommandId === selectedTask.id
                        ? selectedTask.command
                        : maskCommandPreview(selectedTask.command)}
                    </code>
                    {revealedCommandId === selectedTask.id ? (
                      <button type="button" onClick={() => setRevealedCommandId(undefined)}>
                        Hide
                      </button>
                    ) : (
                      <button type="button" onClick={() => void revealCommand(selectedTask)}>
                        Reveal command
                      </button>
                    )}
                  </div>
                </div>
              </div>
              <div className="log-section">
                <div className="log-heading">Logs</div>
                <pre className="log-view" ref={logRef}>{selectedLog || 'Start the selected task to stream logs here.'}</pre>
              </div>
            </section>
          )}
        </div>
      </section>
      {isTaskModalOpen && (
        <div className="modal-backdrop" role="presentation" onMouseDown={() => setIsTaskModalOpen(false)}>
          <section className="modal task-modal" role="dialog" aria-modal="true" aria-label="New task" onMouseDown={(event) => event.stopPropagation()}>
            <header className="modal-header">
              <div className="panel-title">
                <Plus size={16} />
                <span>New task</span>
              </div>
              <button type="button" className="icon-button" aria-label="Close new task" onClick={() => setIsTaskModalOpen(false)}>
                <X size={16} />
              </button>
            </header>
            <label>
              Name
              <input value={form.name} onChange={(event) => setForm({ ...form, name: event.target.value })} />
            </label>
            <label>
              Command
              <textarea
                rows={3}
                value={form.command}
                onChange={(event) => setForm({ ...form, command: event.target.value })}
              />
            </label>
            <label>
              Working directory
              <input value={form.cwd} onChange={(event) => setForm({ ...form, cwd: event.target.value })} />
            </label>
            <div className="segmented">
              <button
                type="button"
                className={form.target === 'local' ? 'active' : ''}
                onClick={() => setForm({ ...form, target: 'local' })}
              >
                Local
              </button>
              <button
                type="button"
                className={form.target === 'ssh' ? 'active' : ''}
                onClick={() => setForm({ ...form, target: 'ssh' })}
              >
                SSH
              </button>
            </div>
            {form.target === 'ssh' && (
              <label>
                SSH Host
                <select value={form.sshHost} onChange={(event) => setForm({ ...form, sshHost: event.target.value })}>
                  <option value="">Select host</option>
                  {hosts.map((host) => (
                    <option key={host.alias} value={host.alias}>
                      {host.alias}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <label className="check-row">
              <input
                type="checkbox"
                checked={form.autoRestart}
                onChange={(event) => setForm({ ...form, autoRestart: event.target.checked })}
              />
              Auto restart
            </label>
            <label className="check-row">
              <input
                type="checkbox"
                checked={form.systemAutostart}
                disabled={form.target === 'ssh'}
                onChange={(event) => setForm({ ...form, systemAutostart: event.target.checked })}
              />
              Local system autostart
            </label>
            <div className="modal-actions">
              <button type="button" onClick={() => setIsTaskModalOpen(false)}>
                Cancel
              </button>
              <button type="button" className="primary" onClick={saveTask}>
                <Plus size={16} />
                Save task
              </button>
            </div>
          </section>
        </div>
      )}
      {isTerminalOpen && (
        <div className="modal-backdrop terminal-backdrop" role="presentation" onMouseDown={() => setIsTerminalOpen(false)}>
          <section className="modal terminal-modal" role="dialog" aria-modal="true" aria-label="Terminal" onMouseDown={(event) => event.stopPropagation()}>
            <header className="modal-header">
              <div className="panel-title">
                <SquareTerminal size={16} />
                <span>Terminal</span>
              </div>
              <button type="button" className="icon-button" aria-label="Close terminal" onClick={() => setIsTerminalOpen(false)}>
                <X size={16} />
              </button>
            </header>
              <div className="terminal-panel">
                <div className="terminal-tabs">
                  {tabs.map((tab) => (
                    <div key={tab.key} className={`terminal-tab ${activeTab === tab.key ? 'active' : ''}`}>
                      <button type="button" className="terminal-tab-select" onClick={() => setActiveTab(tab.key)}>
                        <Bot size={14} />
                        {tab.title}
                      </button>
                      <button type="button" className="terminal-tab-close" aria-label={`Close ${tab.title}`} onClick={() => void closeTerminal(tab)}>
                        <X size={13} />
                      </button>
                    </div>
                  ))}
                  <button type="button" className="icon-button" aria-label="Open local terminal" onClick={() => void openTerminal()}>
                    <Plus size={14} />
                  </button>
                </div>
                <div className="terminal-surfaces">
                  {tabs.length === 0 && (
                    <div className="terminal-empty">
                      <SquareTerminal size={32} />
                      <span>Open a local shell or SSH host to start a terminal session.</span>
                    </div>
                  )}
                  {tabs.map((tab) => (
                    <TerminalSurface key={tab.key} tab={tab} active={activeTab === tab.key} />
                  ))}
                </div>
              </div>
          </section>
        </div>
      )}
    </main>
  )
}

function TerminalSurface({ tab, active }: { tab: TerminalTab; active: boolean }) {
  const containerRef = useRef<HTMLDivElement | null>(null)
  const fitRef = useRef<FitAddon | null>(null)

  useEffect(() => {
    if (!containerRef.current) return
    const terminal = new Terminal({
      cursorBlink: true,
      convertEol: true,
      fontFamily: 'JetBrains Mono, SFMono-Regular, Menlo, Consolas, monospace',
      fontSize: 13,
      theme: {
        background: '#101417',
        foreground: '#d7dde2',
        cursor: '#f4b860',
      },
    })
    const fit = new FitAddon()
    fitRef.current = fit
    terminal.loadAddon(fit)
    terminal.open(containerRef.current)
    requestAnimationFrame(() => fit.fit())

    const onResize = () => fit.fit()
    window.addEventListener('resize', onResize)

    terminal.writeln(`Connected to ${tab.title}`)

    let inputDisposable: ReturnType<typeof terminal.onData> | undefined
    let timer: number | undefined

    if (!tab.backendId) {
      terminal.writeln('Starting session...')
    } else {
      inputDisposable = terminal.onData((data) => {
        void api.ptyWrite(tab.backendId!, data)
      })
      timer = window.setInterval(async () => {
        const snapshot = await api.ptyRead(tab.backendId!).catch((error) => {
          terminal.writeln(errorMessage(error))
          return undefined
        })
        if (!snapshot) return
        if (snapshot.output) terminal.write(snapshot.output)
        if (!snapshot.running) {
          terminal.writeln('\r\nSession ended.')
          window.clearInterval(timer)
        }
      }, 120)
    }

    return () => {
      window.removeEventListener('resize', onResize)
      inputDisposable?.dispose()
      window.clearInterval(timer)
      terminal.dispose()
      fitRef.current = null
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab.key, tab.backendId])

  // Re-fit whenever this tab becomes the active one
  useEffect(() => {
    if (!active) return
    requestAnimationFrame(() => fitRef.current?.fit())
  }, [active])

  return (
    <div
      className="terminal-surface"
      ref={containerRef}
      style={{ display: active ? 'block' : 'none' }}
    />
  )
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

function maskCommandPreview(command: string) {
  const parts = command.trim().split(/\s+/).filter(Boolean)
  if (parts.length === 0) return ''
  if (parts.length === 1) return parts[0]
  const rest = parts.slice(1).join(' ')
  return `${parts[0]} ${'*'.repeat(Math.max(4, rest.length))}`
}

export default App
