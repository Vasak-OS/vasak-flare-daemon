//! Que el cartel no vuelva a destruirse al quedar en silencio.
//!
//! La fuga era ésta: el banner es un webview re-parentado en una ventana de
//! capa, y destruirlo tras un rato de inactividad dejaba vivo su
//! `WebKitWebProcess` —wry/GTK no lo reapea—. Una sesión real juntaba decenas de
//! procesos colgados. El arreglo: el cartel se crea una vez, se **esconde** entre
//! notificaciones y se **reusa**; sólo el vigilante de arranque destruye un
//! webview que nunca cargó.
//!
//! Es una guarda de fuente, no de comportamiento: la fuga es de ciclo de vida de
//! GTK/WebKit y de reaping de procesos, que no se puede montar sin un display ni
//! un WebKit reales. Lo que sí se puede impedir es que el patrón que fugaba
//! vuelva al código. Se verificó además midiendo: con el arreglo, flare-daemon
//! tiene un solo WebKitWebProcess en reposo, no decenas.

const BANNER: &str = include_str!("../src/banner.rs");
const LIB: &str = include_str!("../src/lib.rs");

#[test]
fn no_vuelve_el_desarme_por_inactividad() {
    assert!(
        !BANNER.contains("schedule_teardown"),
        "volvió el desarme por inactividad (`schedule_teardown`): cada ciclo \
         crear→destruir del webview re-parentado fuga un WebKitWebProcess. El \
         cartel se esconde y se reusa; no se destruye al quedar en silencio."
    );
    assert!(
        !BANNER.contains("fn idle("),
        "volvió el plazo de inactividad (`idle`): sólo existía para el desarme \
         que fugaba."
    );
}

#[test]
fn esconder_el_cartel_no_lo_destruye() {
    let desde = LIB.find("fn hide_banner").expect("falta hide_banner");
    let cuerpo = &LIB[desde..];
    let hasta = cuerpo
        .find("\n}\n")
        .expect("no se encontró el fin de hide_banner");
    let cuerpo = &cuerpo[..hasta];
    assert!(
        cuerpo.contains("hide"),
        "hide_banner tiene que esconder la ventana de capa."
    );
    assert!(
        !cuerpo.contains("destroy") && !cuerpo.contains("teardown"),
        "hide_banner no debe destruir ni desarmar el cartel: esconderlo alcanza \
         y reusar el webview evita la fuga y la ventana de carga."
    );
}
