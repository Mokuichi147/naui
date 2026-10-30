//! Creates a reg-free WinRT manifest from `package.appxfragment`.
//!
//! In a framework package, the package manifest tells the OS which WinRT class lives
//! in which DLL. Bundled DLLs have no such manifest, so the same mapping is written
//! in the app's manifest (`winrtv1:activatableClass`).
//!
//! The conversion is the same as the one MSBuild's self-contained deployment
//! performs (`GenerateAppManifestFromAppxFragments` in
//! `Microsoft.WindowsAppSDK.SelfContained.targets`).

use std::fmt::Write as _;

const APPX_NAMESPACE: &str = "http://schemas.microsoft.com/appx/manifest/foundation/windows10";

/// The DLLs containing proxy stubs that are not put in the manifest.
///
/// They are excluded, as MSBuild does. Push notifications need a resident process
/// that cannot be used when bundled, and Widgets have not been verified by Microsoft
/// to work when bundled.
const SKIPPED_PROXY_STUBS: &[&str] = &[
    "PushNotificationsLongRunningTask.ProxyStub.dll",
    "Microsoft.Windows.Widgets.dll",
];

/// Combines the fragments (XML bodies) into a manifest in the order given.
pub(crate) fn generate(fragments: &[&str]) -> Result<String, String> {
    let mut out = String::from(
        "<?xml version='1.0' encoding='utf-8' standalone='yes'?>\n\
         <assembly manifestVersion='1.0'\n    \
         xmlns:asmv3='urn:schemas-microsoft-com:asm.v3'\n    \
         xmlns:winrtv1='urn:schemas-microsoft-com:winrt.v1'\n    \
         xmlns='urn:schemas-microsoft-com:asm.v1'>\n",
    );
    for fragment in fragments {
        let document = roxmltree::Document::parse(fragment)
            .map_err(|e| format!("cannot read package.appxfragment: {e}"))?;
        let extensions = document
            .root_element()
            .children()
            .filter(|node| is(node, "Extensions"))
            .flat_map(|node| node.children())
            .filter(|node| is(node, "Extension"))
            .flat_map(|node| node.children());

        // MSBuild はクラスを先に、プロキシスタブを後にまとめて書く。
        let (servers, stubs): (Vec<_>, Vec<_>) = extensions
            .filter(|node| is(node, "InProcessServer") || is(node, "ProxyStub"))
            .partition(|node| is(node, "InProcessServer"));

        for server in servers {
            let path = child_text(&server, "Path")?;
            let _ = writeln!(out, "    <asmv3:file name='{}'>", escape(path));
            for class in server
                .children()
                .filter(|node| is(node, "ActivatableClass"))
            {
                let id = attribute(&class, "ActivatableClassId")?;
                let _ = writeln!(
                    out,
                    "        <winrtv1:activatableClass name='{}' threadingModel='both'/>",
                    escape(id)
                );
            }
            out.push_str("    </asmv3:file>\n");
        }

        for stub in stubs {
            let path = child_text(&stub, "Path")?;
            if SKIPPED_PROXY_STUBS.contains(&path) {
                continue;
            }
            let class_id = attribute(&stub, "ClassId")?;
            let _ = writeln!(out, "    <asmv3:file name='{}'>", escape(path));
            let _ = writeln!(
                out,
                "        <asmv3:comClass clsid='{{{}}}'/>",
                escape(class_id)
            );
            for interface in stub.children().filter(|node| is(node, "Interface")) {
                let name = attribute(&interface, "Name")?;
                let iid = attribute(&interface, "InterfaceId")?;
                let _ = writeln!(
                    out,
                    "        <asmv3:comInterfaceProxyStub name='{}' iid='{{{}}}'/>",
                    escape(name),
                    escape(iid)
                );
            }
            out.push_str("    </asmv3:file>\n");
        }
    }
    out.push_str("</assembly>\n");
    Ok(out)
}

fn is(node: &roxmltree::Node, name: &str) -> bool {
    node.is_element()
        && node.tag_name().name() == name
        && node.tag_name().namespace() == Some(APPX_NAMESPACE)
}

fn child_text<'a>(node: &roxmltree::Node<'a, '_>, name: &str) -> Result<&'a str, String> {
    node.children()
        .find(|child| is(child, name))
        .and_then(|child| child.text())
        .map(str::trim)
        .ok_or_else(|| format!("package.appxfragment has no {name}"))
}

fn attribute<'a>(node: &roxmltree::Node<'a, '_>, name: &str) -> Result<&'a str, String> {
    node.attribute(name)
        .ok_or_else(|| format!("package.appxfragment has no {name} attribute"))
}

/// Makes it writable as an attribute value enclosed in single quotes.
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&apos;")
}
