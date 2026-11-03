import EdgesCore
import Foundation
import Security

/// The iOS edge's secure storage. The core decides what to keep (the ChatGPT
/// session); the Keychain keeps it safe. UniFFI generated the protocol.
final class Keychain: SecureStorage {
    private func query(_ key: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: "dev.tuist.edges",
            kSecAttrAccount as String: key,
        ]
    }

    func read(key: String) -> String? {
        var request = query(key)
        request[kSecReturnData as String] = true
        var item: CFTypeRef?
        guard SecItemCopyMatching(request as CFDictionary, &item) == errSecSuccess, let data = item as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }

    func write(key: String, value: String) {
        delete(key: key)
        var item = query(key)
        item[kSecValueData as String] = Data(value.utf8)
        item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        SecItemAdd(item as CFDictionary, nil)
    }

    func delete(key: String) {
        SecItemDelete(query(key) as CFDictionary)
    }
}
