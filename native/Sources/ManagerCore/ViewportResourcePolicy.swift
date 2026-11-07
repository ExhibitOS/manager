// SPDX-License-Identifier: Apache-2.0
import Foundation

public enum ViewportResourcePolicy {
    /// Exact compiled registered material maps. Neither exhibition data nor a
    /// caller-supplied directory can extend this WebKit network exception.
    private static let surfaceMaps = [
        "surface-material-textures/museum-matte-paint/1/color-01fee8fd9374.png",
        "surface-material-textures/museum-matte-paint/1/normal-10246edc0ab3.png",
        "surface-material-textures/museum-matte-paint/1/roughness-b62c048f8819.png",
        "surface-material-textures/fine-lime-plaster/1/color-73294b172675.png",
        "surface-material-textures/fine-lime-plaster/1/normal-36d7e70a9317.png",
        "surface-material-textures/fine-lime-plaster/1/roughness-bb774369bf82.png",
        "surface-material-textures/fine-gallery-concrete/1/color-634a67d2a076.png",
        "surface-material-textures/fine-gallery-concrete/1/normal-124a39f69958.png",
        "surface-material-textures/fine-gallery-concrete/1/roughness-d773c26d56f0.png"
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
    private static let privateFloorMaps = [
        "floor-textures/quiet-oak-herringbone/1/color-4e001eaaba46.png",
        "floor-textures/quiet-oak-herringbone/1/normal-21bddb22f10b.png",
        "floor-textures/quiet-oak-herringbone/1/roughness-f889803f9d84.png"
    ]
    /// WK content blockers accept a strict regex subset: anchors occur only at
    /// the start/end, and each resource gets its own rule (no disjunction).
    public static func encodedRules(origin: String, tenantId: String, savedExhibitionId: String? = nil, privateFloor: Bool = false, workspaceMaterials: Bool = false, workspaceMaterialMultimap: Bool = false, workspaceFurnishings: Bool = false) throws -> String {
        guard let url = URL(string: origin), let port = url.port, (1024...65535).contains(port), origin == "http://127.0.0.1:\(port)", let tenant = UUID(uuidString: tenantId)?.uuidString.lowercased() else { throw AuthoringFailure("LOCAL_SCOPE_INVALID") }
        let base = "^http://127\\.0\\.0\\.1:\(port)/"
        let artwork = "api/v1/tenants/\(tenant)/cms/artworks/[a-fA-F0-9-]+"
        func uuidPattern(hex: String, variant: String) -> String {
            [String(repeating: hex, count: 8), String(repeating: hex, count: 4),
             "[1-8]" + String(repeating: hex, count: 3), variant + String(repeating: hex, count: 3),
             String(repeating: hex, count: 12)].joined(separator: "-")
        }
        let exhibitionId = uuidPattern(hex: "[a-fA-F0-9]", variant: "[89aAbB]")
        let viewportCheck = "api/v1/tenants/\(tenant)/studio/exhibitions/\(exhibitionId)/native-viewport-check$"
        var suffixes = ["native-viewport\\.html$", "assets/[A-Za-z0-9_.-]+\\.js$", "assets/[A-Za-z0-9_.-]+\\.css$", "basis/[A-Za-z0-9_.-]+\\.js$", "basis/[A-Za-z0-9_.-]+\\.wasm$", "api/v1/auth/session$", "api/v1/tenants/\(tenant)/studio/artworks/[a-fA-F0-9-]+$", viewportCheck, artwork + "$", artwork + "/display$", artwork + "/preview\\?expectedRevision=[0-9]+$"]
        let workspaceResourceId = uuidPattern(hex: "[a-f0-9]", variant: "[89ab]")
        if workspaceMaterials {
            let version = "[1-9]" + String(repeating: "[0-9]?", count: 9)
            let material = "api/v1/tenants/\(tenant)/workspace/materials/\(workspaceResourceId)/versions/\(version)"
            // These URL exceptions serve the two read-only Platform routes.
            // WebKit filters URLs; the server still enforces methods and ownership.
            suffixes.append(material + "$")
            suffixes += WorkspaceSurfaceMaterialBinding.supportedMapRoles(multimap: workspaceMaterialMultimap)
                .map { material + "/maps/" + $0.rawValue + "$" }
        }
        if workspaceFurnishings {
            let furnishing = "api/v1/tenants/\(tenant)/workspace/furnishings/\(workspaceResourceId)/versions/1"
            // These read routes still enforce the session, owner and display grant on the server.
            suffixes += [furnishing + "/display-check$", furnishing + "/model$"]
        }
        // A capture may recheck only its already-bound saved source. Ordinary
        // viewport policy still refuses all full exhibition reads.
        if let savedExhibitionId {
            guard let id = UUID(uuidString: savedExhibitionId)?.uuidString.lowercased(),
                  id.range(of: "^" + exhibitionId + "$", options: .regularExpression) != nil else { throw AuthoringFailure("LOCAL_SCOPE_INVALID") }
            suffixes.append("api/v1/tenants/\(tenant)/studio/exhibitions/" + NSRegularExpression.escapedPattern(for: id) + "$")
        }
        var rules: [[String:Any]] = [["trigger":["url-filter":".*"],"action":["type":"block"]]]
        let materialRules = (surfaceMaps + floorMaps + (privateFloor ? privateFloorMaps : [])).map { base + NSRegularExpression.escapedPattern(for: $0) + "$" }
        for pattern in suffixes.map({ base + $0 }) + materialRules + ["^blob:", "^data:"] {
            rules.append(["trigger":["url-filter":pattern,"url-filter-is-case-sensitive":true],"action":["type":"ignore-previous-rules"]])
        }
        let data = try JSONSerialization.data(withJSONObject: rules)
        return String(decoding: data, as: UTF8.self)
    }
}
