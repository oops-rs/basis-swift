import Basis
import Foundation

/// The README's snippets, compiled: if the API drifts from what the README
/// shows, this file stops building. Never run.
private func readmeCompiles(repo: URL, key: String, historyDir: String) async throws {
    let workspace = try await Workspace.open(
        path: repo.path,
        options: WorkspaceOptions(provider: .anthropic, apiKey: key, history: .directory(path: historyDir))
    )

    let run = try workspace.prepare()
    let report = try await run.send("what does this repo do?") { event in
        if let text = event.assistantText { print(text, terminator: "") }
    }
    print(report.outcome, report.usage.totalTokens)
    try await run.send("and which part is riskiest?")

    try await run.send("bump the deps", approve: { request in
        request.sideEffectLevel == .localState ? .allow : .deny("this app allows local edits only")
    })

    try await run.send("bump the deps") { event in
        print(event.type)
    } approve: { request in
        request.sideEffectLevel == .localState ? .allow : .deny("this app allows local edits only")
    }

    let control = TurnControl()
    control.stop()
    control.cancel()
    try await run.send("prompt", control: control)

    _ = try workspace.resume(agentId: run.agentId())
}
