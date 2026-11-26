// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

/** The French dictionary: its shape is the contract every other language follows (see `Messages`). */
export const fr = {
  /** BCP 47 tag the `Intl` formatters take. */
  locale: 'fr-FR',
  language: { code: 'FR', switchTo: 'Passer en français' },
  theme: {
    switchToLight: 'Passer au thème clair',
    switchToDark: 'Passer au thème sombre',
    light: 'Thème clair',
    dark: 'Thème sombre',
  },
  status: {
    working: { note: null, phrase: 'au travail' },
    waiting: { note: 'Vous attend', phrase: 'vous attend' },
    blocked: { note: 'Bloqué', phrase: 'bloqué' },
    done: { note: 'Terminé', phrase: 'terminé' },
  },
  filters: {
    groupLabel: 'Filtrer les agents',
    all: 'Tous',
    working: 'Au travail',
    waiting: 'Vous attendent',
    blocked: 'Bloqués',
    done: 'Terminés',
    agentsShown: (count: number) => (count === 1 ? '1 agent affiché' : `${count} agents affichés`),
  },
  steps: {
    state: {
      upcoming: 'à venir',
      current: 'en cours',
      done: 'terminée',
      skipped: 'retirée',
      external: 'hors Atlas',
    },
    finished: 'Terminée',
    pass: (pass: number) => `passe ${pass}`,
    tokensTitle: "Coût en équivalent tokens d'entrée : les relectures du cache comptent pour un dixième",
    tokensSpoken: " tokens, en équivalent tokens d'entrée",
  },
  board: {
    connecting: 'Connexion au daemon...',
    offline: 'Hors ligne, reconnexion...',
    empty: "Aucun agent pour l'instant.",
    canvasLabel: 'Tableau des agents',
    zoomIn: 'Zoomer',
    zoomOut: 'Dézoomer',
    showAll: 'Tout afficher',
  },
  gate: {
    starting: "Démarrage de l'usine...",
    retry: 'Réessayer',
  },
  orchestrator: {
    title: 'Orchestrateur',
    newConversation: 'Nouvelle conversation',
    newConversationFailed: 'Impossible de démarrer une nouvelle conversation.',
    emptyThread: 'Nouvelle conversation. Donnez un ticket, une idée ou une question.',
    thinking: 'Réfléchit',
    thinkingSpoken: "L'orchestrateur réfléchit",
  },
  composer: {
    mentionListLabel: 'Agents à mentionner',
    messageLabel: "Message à l'orchestrateur",
    placeholder: "Écrire à l'orchestrateur, @ pour un agent",
    broadcastPlaceholder: 'Message pour tous les agents',
    broadcast: 'Diffuser à tous',
    send: 'Envoyer',
    sendFailed: "Le message n'est pas parti. Le daemon répond-il ?",
    sendRefused: (reason: string) => `Le message n'est pas parti : ${reason}`,
  },
  message: {
    copy: 'Copier',
    copied: 'Copié',
    openSessionOf: (ticketRef: string) => `Ouvrir la session de ${ticketRef}`,
    cardKind: { launch: 'Lancement', decision: 'Décision', workflow: 'Parcours modifié' },
  },
  actions: {
    archive: 'Archiver',
    failedRetry: 'Échec, réessayer',
    rememberForAll: 'Retenir pour tous les projets',
    rememberForProject: 'Retenir pour le projet',
    openSession: 'Ouvrir la session',
    reopenSession: 'Rouvrir la session',
  },
  catalogue: {
    label: 'Catalogue',
    brick: (id: string) => `Brique ${id}`,
    skill: (name: string) => `Skill ${name}`,
    subagent: (name: string) => `Sous-agent ${name}`,
    disable: (id: string) => `Désactiver la brique ${id}`,
    enable: (id: string) => `Réactiver la brique ${id}`,
    approve: 'Valider',
    refuse: 'Refuser',
    decision: { applied: 'Appliqué', refused: 'Refusé' },
    unreachable: 'Atlas est injoignable, réessayez.',
  },
  session: {
    breadcrumbLabel: "Fil d'Ariane",
    factory: 'Usine',
    since: (elapsed: string) => ` depuis ${elapsed}`,
    viewMergeRequest: 'Voir la MR',
    resume: 'Relancer',
    resumeFailed: "La relance n'a pas abouti.",
    close: 'Fermer',
    closeFailed: "La fermeture n'a pas abouti.",
    panesLabel: 'Contenu',
    pane: { report: 'Rapport', terminal: 'Terminal' },
    closedNotice: "Session fermée. Relancer la rouvre là où elle s'est arrêtée.",
  },
  terminal: {
    label: "Terminal de l'agent",
    sessionEnded: 'Session terminée.',
  },
};

/** The dictionary of any language: strings are plain `string`, not the French literals. */
export type Messages = typeof fr;
