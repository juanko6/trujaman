#!/usr/bin/env bash
# Doble clic en Finder: arranca trujamán y abre su página en el navegador.
# Para cerrarlo, cierra esta ventana de Terminal.
cd "$(dirname "$0")" && exec scripts/servidor.sh
