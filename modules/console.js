// modules/console.js
// Implementación de la API console para cumplir con WinterCG / WinterTC.
// Utiliza la función nativa __log para impresión directa a la salida estándar sin copias ni serialización.

(function() {
    if (!globalThis.console) {
        globalThis.console = {
            log: function(...args) {
                globalThis.__log(...args);
            },
            info: function(...args) {
                globalThis.__log(...args);
            },
            warn: function(...args) {
                globalThis.__log(...args);
            },
            error: function(...args) {
                globalThis.__log(...args);
            }
        };
    }
})();
