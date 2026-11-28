# Fish dans les terminaux de Ferrite : couleurs de la charte, accueil en français et
# complétion simple. Les terminaux du bureau ouvrent fish (voir ferrite-session et le
# Terminal de ferrite-apps). Rien ici ne s'applique hors d'un shell interactif.
status is-interactive; or return

# Couleurs : posées une seule fois, en variables universelles, avant celles de fish
# (qui ne remplace pas une couleur déjà posée). On peut ensuite les changer avec
# fish_config sans qu'elles reviennent. Noms ANSI : le Terminal les adapte au thème.
if not set -q __ferrite_couleurs
    set -U fish_color_normal normal
    set -U fish_color_command blue --bold
    set -U fish_color_keyword magenta --bold
    set -U fish_color_param cyan
    set -U fish_color_option cyan
    set -U fish_color_quote yellow
    set -U fish_color_redirection magenta
    set -U fish_color_end magenta
    set -U fish_color_operator brcyan
    set -U fish_color_escape brcyan
    set -U fish_color_error red --bold
    set -U fish_color_comment brblack --italics
    set -U fish_color_autosuggestion brblack
    set -U fish_color_valid_path --underline
    set -U fish_color_user green --bold
    set -U fish_color_host normal
    set -U fish_color_host_remote yellow
    set -U fish_color_cwd blue
    set -U fish_color_cwd_root red
    set -U fish_color_status red
    set -U fish_color_selection white --bold --background=brblack
    set -U fish_color_search_match --background=brblack
    # Liste des complétions (sous la ligne, après Tab)
    set -U fish_pager_color_prefix cyan --bold --underline
    set -U fish_pager_color_completion normal
    set -U fish_pager_color_description brblack --italics
    set -U fish_pager_color_progress brwhite --background=blue
    set -U fish_pager_color_selected_background --background=brblack
    set -U __ferrite_couleurs 1
end

function fish_greeting
    set_color --bold blue
    echo -n "Terminal Ferrite"
    set_color normal
    echo " — astuces :"
    set_color brblack
    echo "  Tab      compléter (accepte aussi la suggestion grise)"
    echo "  →        accepter la suggestion grise"
    echo "  ↑        commandes précédentes commençant par ce qui est tapé"
    echo "  Alt+H    aide de la commande en cours"
    set_color normal
end

# Tab : accepte la suggestion grise quand il y en a une et que le curseur est en fin
# de ligne, sinon complète comme d'habitude.
function __ferrite_tab
    if commandline --showing-suggestion; and test (commandline --cursor) -eq (string length -- (commandline))
        commandline -f accept-autosuggestion
    else
        commandline -f complete
    end
end
bind tab __ferrite_tab
bind -M insert tab __ferrite_tab
