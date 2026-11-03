import Foundation
import Security

/// The iOS edge's platform service for secrets: the core returns the session,
/// and the edge keeps it in the Keychain.
enum SecureStore {
    private static let query: [String: Any] = [
        kSecClass as String: kSecClassGenericPassword,
        kSecAttrService as String: "dev.tuist.edges",
        kSecAttrAccount as String: "session",
    ]

    static func read() -> [String: Any]? {
        var item: CFTypeRef?
        var request = query
        request[kSecReturnData as String] = true
        guard SecItemCopyMatching(request as CFDictionary, &item) == errSecSuccess, let data = item as? Data else { return nil }
        return try? JSONSerialization.jsonObject(with: data) as? [String: Any]
    }

    static func write(_ session: [String: Any]) {
        guard let data = try? JSONSerialization.data(withJSONObject: session) else { return }
        clear()
        var item = query
        item[kSecValueData as String] = data
        item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        SecItemAdd(item as CFDictionary, nil)
    }

    static func clear() {
        SecItemDelete(query as CFDictionary)
    }
}
