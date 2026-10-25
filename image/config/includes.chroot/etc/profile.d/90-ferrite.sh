# Lance la session Ferrite à la connexion sur tty1 (une seule fois par connexion).
case "$-" in
    *i*) ;;
    *) return 0 ;;
esac

if [ "$(tty 2>/dev/null)" = /dev/tty1 ] && [ -z "${FERRITE_SESSION:-}" ] && [ -x /usr/local/bin/ferrite-session ]; then
    exec /usr/local/bin/ferrite-session
fi
