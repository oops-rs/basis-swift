import Foundation
import Testing

import Basis

/// basis-a2a over a Swift host, through real HTTP: the server mints the id the
/// host is handed, the authenticator's attributes reach the host, a host's own
/// artifacts follow the result, and progress is pulled by cursor.
@Test(.timeLimit(.minutes(1)))
func aSwiftHostServesItsTasksOverA2a() async throws {
    let state = try temporaryDirectory()
    let host = AnsweringHost()
    let server = try await A2aServer.serve(
        options: A2aServeOptions(
            name: "swift-host", description: "Answers in tests.", version: "9.9.9",
            stateDir: state.path, address: "127.0.0.1:0"
        ),
        host: host,
        authenticator: ChainAuthenticator()
    )
    defer { Task { await server.shutdown() } }
    let endpoint = try #require(URL(string: server.endpoint()))

    let card = try await getJSON(endpoint.appendingPathComponent(".well-known/agent-card.json"))
    #expect(card["name"] as? String == "swift-host")
    #expect(card["version"] as? String == "9.9.9")

    var request = URLRequest(url: endpoint)
    request.httpMethod = "POST"
    request.setValue("application/json", forHTTPHeaderField: "Content-Type")
    request.setValue("1.0", forHTTPHeaderField: "A2A-Version")
    request.setValue("Bearer as:alice", forHTTPHeaderField: "Authorization")
    request.setValue("gateway>alice", forHTTPHeaderField: "X-Chain")
    request.httpBody = Data(#"""
    {"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{"message":{
      "messageId":"m1","role":"ROLE_USER","parts":[{"text":"hello"}]}}}
    """#.utf8)
    let (body, response) = try await URLSession.shared.data(for: request)
    #expect((response as? HTTPURLResponse)?.statusCode == 200)
    let task = try #require(
        ((try JSONSerialization.jsonObject(with: body) as? [String: Any])?["result"] as? [String: Any])?["task"]
            as? [String: Any]
    )
    let status = try #require(task["status"] as? [String: Any])
    #expect(status["state"] as? String == "TASK_STATE_COMPLETED")
    let artifacts = try #require(task["artifacts"] as? [[String: Any]])
    #expect(artifacts.map { $0["artifactId"] as? String } == ["result", "notes"])

    let start = try #require(host.starts.first)
    #expect(start.taskId == task["id"] as? String)
    #expect(start.principal == "alice")
    #expect(start.attributes == ["chain": "gateway>alice"])
    #expect(start.prompt == "hello")

    request.setValue(nil, forHTTPHeaderField: "Authorization")
    let (_, refused) = try await URLSession.shared.data(for: request)
    #expect((refused as? HTTPURLResponse)?.statusCode == 401)
}

private func getJSON(_ url: URL) async throws -> [String: Any] {
    let (data, _) = try await URLSession.shared.data(from: url)
    return try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
}

private final class AnsweringHost: A2aTaskHost, @unchecked Sendable {
    private let lock = NSLock()
    private var started: [A2aHostedStart] = []

    var starts: [A2aHostedStart] { lock.withLock { started } }

    func start(start: A2aHostedStart) async throws -> String {
        lock.withLock { started.append(start) }
        return "host-\(start.taskId)"
    }

    func standing(id: String) async throws -> A2aHostedStanding {
        .completed(parts: [A2aPart(content: .text(text: "done"))])
    }

    func cancel(id: String) async throws {}
    func settled(id: String) async throws {}

    func reply(id: String, reply: A2aHostedReply) async throws {
        throw A2aHostError.Refused(message: "no questions")
    }

    func artifacts(id: String) async throws -> [A2aArtifact] {
        [A2aArtifact(
            artifactId: "notes", name: "notes.md",
            parts: [A2aPart(content: .text(text: "# Notes"), mediaType: "text/markdown")]
        )]
    }

    func progress(id: String, cursor: UInt64?) async throws -> A2aProgressNote? { nil }
}

private final class ChainAuthenticator: A2aAuthenticator {
    func authenticate(head: A2aRequestHead) async throws -> A2aPrincipal {
        let value = { (name: String) in head.headers.first { $0.name == name }?.value }
        guard let name = value("authorization")?.split(separator: ":").last.map(String.init),
              value("authorization")?.hasPrefix("Bearer as:") == true
        else { throw A2aRefusal.Unauthenticated }
        var attributes: [String: String] = [:]
        if let chain = value("x-chain") { attributes["chain"] = chain }
        return A2aPrincipal(name: name, attributes: attributes)
    }

    func security() -> A2aSecurity {
        A2aSecurity(
            schemeName: "gateway",
            scheme: .httpAuth(scheme: "Bearer", description: nil, bearerFormat: nil),
            challenge: "Bearer"
        )
    }
}
