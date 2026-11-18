#!/usr/bin/env bash
# Doble clic en Finder: arranca trujamán y abre su página en el navegador.
# Se apaga solo al cerrar la pestaña del navegador (o con el botón Cerrar).
cd "$(dirname "$0")" && exec scripts/servidor.sh
