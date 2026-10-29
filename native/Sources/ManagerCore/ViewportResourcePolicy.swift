// SPDX-License-Identifier: Apache-2.0
import Foundation

public enum ViewportResourcePolicy {
    /// Exact compiled first-two-material maps. Neither exhibition data nor a
    /// caller-supplied directory can extend this WebKit network exception.
    private static let surfaceMaps = [
        "surface-material-textures/museum-matte-paint/1/color-01fee8fd9374.png",
        "surface-material-textures/museum-matte-paint/1/normal-10246edc0ab3.png",
        "surface-material-textures/museum-matte-paint/1/roughness-b62c048f8819.png",
        "surface-material-textures/fine-lime-plaster/1/color-73294b172675.png",
        "surface-material-textures/fine-lime-plaster/1/normal-36d7e70a9317.png",
        "surface-material-textures/fine-lime-plaster/1/roughness-bb774369bf82.png"
    ]
    /// Exact registered public floor maps; no caller-provided path or directory wildcard.
    private static let floorMaps = [
        "floor-textures/Carpet004/1/ao-cba7d7a73cc9.png",
        "floor-textures/Carpet004/1/color-df90d6a3be6b.png",
        "floor-textures/Carpet004/1/normal-gl-956a4ad49737.png",
        "floor-textures/Carpet004/1/roughness-6a60aa6770e6.png",
        "floor-textures/Marble006/1/color-d44e0a2b0402.png",
        "floor-textures/Marble006/1/normal-gl-a61a93414470.png",
        "floor-textures/Marble006/1/roughness-cd5898bc49d9.png",
        "floor-textures/Tiles074/1/color-99d35940d5ae.png",
        "floor-textures/Tiles074/1/normal-gl-89ad085427ad.png",
        "floor-textures/Tiles074/1/roughness-538f7d7c0645.png",
        "floor-textures/WoodFloor051/1/color-68bad517cc4a.png",
        "floor-textures/WoodFloor051/1/normal-gl-23d13c247759.png",
        "floor-textures/WoodFloor051/1/roughness-8c578dfb7c61.png"
    ]
    /// WK content blockers accept a strict regex subset: anchors occur only at
    /// the start/end, and each resource gets its own rule (no disjunction).
    public static func encodedRules(origin: String, tenantId: String) throws -> String {
        guard let url = URL(string: origin), let port = url.port, (1024...65535).contains(port), origin == "http://127.0.0.1:\(port)", let tenant = UUID(uuidString: tenantId)?.uuidString.lowercased() else { throw AuthoringFailure("LOCAL_SCOPE_INVALID") }
        let base = "^http://127\\.0\\.0\\.1:\(port)/"
        let artwork = "api/v1/tenants/\(tenant)/cms/artworks/[a-fA-F0-9-]+"
        let suffixes = ["native-viewport\\.html$", "assets/[A-Za-z0-9_.-]+\\.js$", "assets/[A-Za-z0-9_.-]+\\.css$", "basis/[A-Za-z0-9_.-]+\\.js$", "basis/[A-Za-z0-9_.-]+\\.wasm$", "api/v1/auth/session$", "api/v1/tenants/\(tenant)/studio/artworks/[a-fA-F0-9-]+$", artwork + "$", artwork + "/display$", artwork + "/preview\\?expectedRevision=[0-9]+$"]
        var rules: [[String:Any]] = [["trigger":["url-filter":".*"],"action":["type":"block"]]]
        let materialRules = (surfaceMaps + floorMaps).map { base + NSRegularExpression.escapedPattern(for: $0) + "$" }
        for pattern in suffixes.map({ base + $0 }) + materialRules + ["^blob:", "^data:"] {
            rules.append(["trigger":["url-filter":pattern,"url-filter-is-case-sensitive":true],"action":["type":"ignore-previous-rules"]])
        }
        let data = try JSONSerialization.data(withJSONObject: rules)
        return String(decoding: data, as: UTF8.self)
    }
}
