// SPDX-License-Identifier: Apache-2.0
import Foundation
import Security

public struct NativeHostingAccount: Decodable, Identifiable, Sendable {
    public let id: String, name: String
}
public struct NativeHostingWork: Decodable, Sendable {
    public let format: Int, id: String, workspaceId: String, tenantId: String, principalId: String
    public let accountId: String, projectName: String, state: String
    public let source: NativeStaticExportReceipt
    public let deploymentId: String?, publicUrl: String?, errorCode: String?
    public var verifiedURL: URL? {
        guard state == "completed", let publicUrl, publicUrl == "https://" + projectName + ".pages.dev",
              projectName.range(of: "^[a-z0-9](?:[a-z0-9-]{0,57}[a-z0-9])?$", options: .regularExpression) != nil,
              let url = URL(string: publicUrl), url.scheme == "https", url.user == nil, url.password == nil else { return nil }
        return url
    }
}
public struct NativeHostingStatus: Decodable, Sendable {
    public let running: Bool
    public let accounts: [NativeHostingAccount]
    public let work: NativeHostingWork?
    public let errorCode: String?
}
public struct NativeHostingReview: Decodable, Sendable {
    public struct Info: Decodable, Sendable { public let title: String, publicationId: String, projectName: String, revisionSha256: String, indexSha256: String }
    public let source: NativeStaticExportReceipt
    public let info: Info
    public var expectedURL: String { "https://" + info.projectName + ".pages.dev" }
}

/// The same fixed service/account as the existing Manager. No ACL changes,
/// shell commands, provider tokens in preferences, or background secret reads.
public actor CloudflareCredentialStore {
    public init() {}
    private var query: [String: Any] { [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: "org.exhibitos.manager.hosted", kSecAttrAccount as String: "cloudflare"] }
    public func contains() throws -> Bool {
        var attributes = query
        attributes[kSecReturnAttributes as String] = true
        attributes[kSecMatchLimit as String] = kSecMatchLimitOne
        attributes[kSecUseAuthenticationUI as String] = kSecUseAuthenticationUIFail
        let result = SecItemCopyMatching(attributes as CFDictionary, nil)
        if result == errSecItemNotFound { return false }
        guard result == errSecSuccess else { throw BridgeFailure("HOSTED_KEYCHAIN_UNAVAILABLE") }
        return true
    }
    public func save(_ token: String) throws {
        var bytes = Data(token.utf8)
        defer { bytes.resetBytes(in: bytes.startIndex..<bytes.endIndex) }
        guard !bytes.isEmpty, bytes.count <= 4096, bytes.allSatisfy({ (33...126).contains($0) }) else { throw BridgeFailure("HOSTED_TOKEN_INVALID") }
        let result = SecItemUpdate(query as CFDictionary, [kSecValueData as String: bytes] as CFDictionary)
        if result == errSecItemNotFound {
            var item = query; item[kSecValueData as String] = bytes
            guard SecItemAdd(item as CFDictionary, nil) == errSecSuccess else { throw BridgeFailure("HOSTED_KEYCHAIN_STORE_FAILED") }
        } else if result != errSecSuccess { throw BridgeFailure("HOSTED_KEYCHAIN_STORE_FAILED") }
    }
    /// Only call after the user's explicit account-check, deploy or reconcile action.
    public func load() throws -> String {
        var item = query
        item[kSecReturnData as String] = true; item[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        guard SecItemCopyMatching(item as CFDictionary, &result) == errSecSuccess, var bytes = result as? Data else { throw BridgeFailure("HOSTED_KEYCHAIN_UNAVAILABLE") }
        defer { bytes.resetBytes(in: bytes.startIndex..<bytes.endIndex) }
        guard !bytes.isEmpty, bytes.count <= 4096, bytes.allSatisfy({ (33...126).contains($0) }), let token = String(data: bytes, encoding: .utf8) else { throw BridgeFailure("HOSTED_TOKEN_INVALID") }
        return token
    }
}
