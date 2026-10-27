import Foundation
import Testing

@testable import Basis

@Test func eventReadsTheJsonlLine() throws {
    let event = try Event(line: #"{"seq":3,"type":"assistant_delta","text":"hel"}"#)
    #expect(event.seq == 3)
    #expect(event.type == "assistant_delta")
    #expect(event.assistantText == "hel")
}

@Test func eventOtherThanAssistantHasNoText() throws {
    let event = try Event(line: #"{"seq":0,"type":"tool_started","tool_call_id":"c","tool_name":"read"}"#)
    #expect(event.assistantText == nil)
}

@Test func aMissingConfigFileFailsTheOpen() async throws {
    let dir = try temporaryDirectory()
    await #expect(throws: BasisError.self) {
        _ = try await Workspace.open(
            path: dir.path,
            options: WorkspaceOptions(configFile: dir.appendingPathComponent("absent.json").path)
        )
    }
}

/// The whole lifecycle against an endpoint nothing listens on: the open, the
/// mint, the async send through the Rust runtime, and back as a thrown error
/// or a failed report — never a hang or a crash.
@Test(.timeLimit(.minutes(2)))
func aTurnAgainstADeadEndpointComesBack() async throws {
    let dir = try temporaryDirectory()
    let workspace = try await Workspace.open(
        path: dir.path,
        options: WorkspaceOptions(
            provider: .openAi,
            model: "test-model",
            baseUrl: "http://127.0.0.1:9/v1",
            apiKey: "unused",
            history: .ephemeral
        )
    )
    let run = try workspace.prepare()
    #expect(!run.agentId().isEmpty)

    let events = Events()
    do {
        let report = try await run.send("hello") { events.append($0) }
        #expect(report.outcome != .ok)
    } catch is BasisError {
        // A turn that never reached the provider may throw instead.
    }
    if let first = events.all.first {
        #expect(first.type == "run_started")
    }
}

private final class Events: @unchecked Sendable {
    private let lock = NSLock()
    private var events: [Event] = []

    func append(_ event: Event) {
        lock.withLock { events.append(event) }
    }

    var all: [Event] {
        lock.withLock { events }
    }
}

private func temporaryDirectory() throws -> URL {
    let dir = FileManager.default.temporaryDirectory
        .appendingPathComponent("basis-swift-tests-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    return dir
}
