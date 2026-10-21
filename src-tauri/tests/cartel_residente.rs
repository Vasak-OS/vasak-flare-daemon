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
const COMMANDS: &str = include_str!("../src/commands.rs");

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
fn recupera_el_cartel_si_su_web_process_muere() {
    // El vigilante de arranque sólo cubre que el webview nunca cargue. Un crash
    // del WebKitWebProcess ya cargado dejaría el cartel mudo con READY en true;
    // esto lo recupera recargando.
    assert!(
        COMMANDS.contains("connect_web_process_terminated"),
        "falta la recuperación: sin `web-process-terminated`, si el proceso del \
         cartel muere a mitad de sesión queda roto hasta reiniciar el servicio."
    );
}

#[test]
fn esconder_el_cartel_no_lo_destruye() {
    let desde = COMMANDS.find("fn hide_banner").expect("falta hide_banner");
    let cuerpo = &COMMANDS[desde..];
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

/// Cuerpo de `fn <nombre>` en `COMMANDS`, sin las líneas de comentario: los
/// comentarios explican por qué algo NO está y nombran justo lo prohibido.
fn cuerpo_sin_comentarios(nombre: &str) -> String {
    let desde = COMMANDS
        .find(&format!("fn {nombre}("))
        .unwrap_or_else(|| panic!("falta {nombre}"));
    let cuerpo = &COMMANDS[desde..];
    let hasta = cuerpo
        .find("\n}\n")
        .unwrap_or_else(|| panic!("no se encontró el fin de {nombre}"));
    cuerpo[..hasta]
        .lines()
        .filter(|linea| !linea.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn el_cartel_no_apaga_la_aceleracion_por_hardware() {
    // Con `HardwareAccelerationPolicy::Never` el webview renderizaba por
    // software y su superficie de capa transparente no presentaba ningún pixel:
    // la ventana quedaba mapeada en el compositor (420×92, capa overlay) y el
    // DOM maquetaba, pero el cartel no se veía. Es una guarda de fuente: la
    // presentación del buffer necesita un display y un WebKitGTK reales.
    let cuerpo = cuerpo_sin_comentarios("tune_webkit");
    assert!(
        !cuerpo.contains("set_hardware_acceleration_policy")
            && !cuerpo.contains("HardwareAccelerationPolicy"),
        "tune_webkit volvió a tocar la política de aceleración por hardware: \
         con `Never` el cartel (superficie de capa transparente) se mapea pero \
         no se ve. Dejala en su valor por omisión."
    );
}
