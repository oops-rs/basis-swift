import Foundation

extension Workspace {
    /// Opens the workspace at `path` with basis's defaults.
    public static func open(path: String) async throws -> Workspace {
        try await open(path: path, options: WorkspaceOptions())
    }
}

extension Run {
    /// Sends `prompt` with closures in place of an observer and an approver.
    ///
    /// `onEvent` is called on a basis thread, in order, for every event of the
    /// turn; hop to your own actor before touching UI. `approve` is asked
    /// about each consequential tool call while the turn waits; leaving it out
    /// allows every call, and throwing from it denies.
    @discardableResult
    public func send(
        _ prompt: String,
        control: TurnControl? = nil,
        approve: (@Sendable (ApprovalRequest) async throws -> ApprovalAnswer)? = nil,
        onEvent: (@Sendable (Event) -> Void)? = nil
    ) async throws -> RunReport {
        try await send(
            prompt: prompt,
            observer: onEvent.map(ClosureObserver.init),
            approver: approve.map(ClosureApprover.init),
            control: control
        )
    }
}

extension ApprovalAnswer {
    public static let allow = ApprovalAnswer(decision: .allow)
    public static let allowForSession = ApprovalAnswer(decision: .allowForSession)

    /// A refusal; `reason` is what the model reads as the call's result.
    public static func deny(_ reason: String? = nil) -> ApprovalAnswer {
        ApprovalAnswer(decision: .deny, reason: reason)
    }
}

private final class ClosureObserver: EventObserver {
    private let body: @Sendable (Event) -> Void

    init(_ body: @escaping @Sendable (Event) -> Void) {
        self.body = body
    }

    func onEvent(line: String) throws {
        body(try Event(line: line))
    }
}

private final class ClosureApprover: Approver {
    private let body: @Sendable (ApprovalRequest) async throws -> ApprovalAnswer

    init(_ body: @escaping @Sendable (ApprovalRequest) async throws -> ApprovalAnswer) {
        self.body = body
    }

    func approve(request: ApprovalRequest) async throws -> ApprovalAnswer {
        try await body(request)
    }
}
