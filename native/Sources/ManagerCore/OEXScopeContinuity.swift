// SPDX-License-Identifier: Apache-2.0
/// A busy fresh owner temporarily withholds runtime status while it holds the
/// Session. This preserves only the import scope, not runtime readiness.
public func oexScopeInterrupted(ready: Bool, freshOperation: Bool, importBusy: Bool) -> Bool {
    !ready && !(freshOperation && importBusy)
}
