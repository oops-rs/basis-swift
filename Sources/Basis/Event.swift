import Foundation

/// One event from a run: exactly the line `basis spawn --json` writes.
///
/// `type` names the event (`run_started`, `assistant_delta`, `tool_queued`, …)
/// and the line's other fields depend on it. Decode the line into a
/// `Decodable` of your own for the fields you read: the stream's shape is
/// basis's versioned JSONL schema, announced by `run_started`'s `schema`.
public struct Event: Sendable, Hashable {
    /// Position in this turn's stream, from zero.
    public let seq: UInt64
    public let type: String
    /// The whole line, as basis wrote it.
    public let line: String

    public init(line: String) throws {
        let head = try JSONDecoder().decode(Head.self, from: Data(line.utf8))
        self.seq = head.seq
        self.type = head.type
        self.line = line
    }

    /// Decodes the line into `T`.
    public func decode<T: Decodable>(_: T.Type, using decoder: JSONDecoder = JSONDecoder()) throws -> T {
        try decoder.decode(T.self, from: Data(line.utf8))
    }

    /// The text of an `assistant_delta` or `assistant_message`, the two a
    /// chat view renders; nil for every other event.
    public var assistantText: String? {
        guard type == "assistant_delta" || type == "assistant_message" else { return nil }
        return try? decode(Text.self).text
    }

    private struct Head: Decodable {
        let seq: UInt64
        let type: String
    }

    private struct Text: Decodable {
        let text: String
    }
}
