import { useEffect } from 'react';

const french: Record<string, string> = {
  'Overview': 'Vue d’ensemble', 'Devices': 'Appareils', 'Display': 'Écran', 'Streaming': 'Diffusion', 'Input': 'Commandes', 'Security': 'Sécurité', 'Network': 'Réseau', 'Advanced': 'Avancé', 'About': 'À propos',
  'Ready': 'Prêt', 'Stopped': 'Arrêté', 'Local network only': 'Réseau local uniquement', 'Remote access': 'Accès distant', 'Stop service': 'Arrêter le service', 'Start service': 'Démarrer le service', 'Connect Device': 'Connecter un appareil',
  'Starting Vision-Pro…': 'Démarrage de Vision-Pro…', 'Settings saved.': 'Réglages enregistrés.',
  'The device store failed its integrity check. Every device now requires confirmation.': 'Le contrôle d’intégrité du registre des appareils a échoué. Chaque appareil nécessite désormais une confirmation.',
  'Status': 'État', 'Connected': 'Connecté', 'Service stopped': 'Service arrêté', 'Disconnect': 'Déconnecter', 'Connected device': 'Appareil connecté', 'None': 'Aucun', 'Latency': 'Latence', 'Protected': 'Protégé', 'Local Only': 'Local uniquement', 'Remote Access': 'Accès distant',
  'Vision-Pro is waiting securely on your private network.': 'Vision-Pro attend de manière sécurisée sur votre réseau privé.', 'Start the service to make this PC available.': 'Démarrez le service pour rendre ce PC disponible.', 'No active stream': 'Aucun flux actif',
  'Live streaming': 'Diffusion en direct', 'Windows desktop': 'Bureau Windows', 'Hardware': 'Matériel', 'Software': 'Logiciel', 'Input processing': 'Traitement des commandes',
  'Recent security activity': 'Activité de sécurité récente', 'Security-relevant events from this PC': 'Événements de sécurité de ce PC', 'No security events yet.': 'Aucun événement de sécurité.',
  'No paired devices': 'Aucun appareil associé', 'Choose Connect Device to pair an iPad. Discovery never grants access automatically.': 'Choisissez Connecter un appareil pour associer un iPad. La découverte ne donne jamais accès automatiquement.',
  'Last connected': 'Dernière connexion', 'Never connected': 'Jamais connecté', 'Online': 'En ligne', 'Permission': 'Autorisation', 'View & Control': 'Voir et contrôler', 'View Only': 'Lecture seule', 'Last address': 'Dernière adresse', 'Key': 'Clé', 'Unavailable': 'Indisponible', 'Trust Device': 'Faire confiance', 'Rename': 'Renommer', 'Confirm device removal': 'Confirmer la suppression', 'Remove device': 'Supprimer l’appareil', 'Device name': 'Nom de l’appareil',
  'Choose what Vision-Pro streams by default.': 'Choisissez ce que Vision-Pro diffuse par défaut.', 'Default display': 'Écran par défaut', 'Primary': 'Principal', 'Use': 'Utiliser', 'Power': 'Alimentation', 'Prevent sleep while connected': 'Empêcher la veille pendant la connexion', 'Keeps the PC available during an active session': 'Maintient le PC disponible pendant une session active',
  'Automatic mode negotiates the best format supported by the GPU and iPad.': 'Le mode automatique négocie le meilleur format pris en charge par le GPU et l’iPad.', '4K is the default. Vision-Pro uses a direct local WebRTC path and applies the best format actually supported by the GPU and iPad.': 'La 4K est le réglage par défaut. Vision-Pro utilise un chemin WebRTC local direct et applique le meilleur format réellement pris en charge par le GPU et l’iPad.', 'Performance': 'Performances', 'Resolution': 'Définition', 'Encoder': 'Encodeur', 'Hardware only': 'Matériel uniquement', 'Profile': 'Profil', 'Balanced': 'Équilibré', 'Low Latency': 'Faible latence', 'Maximum Quality': 'Qualité maximale', 'Battery Saver': 'Économie de batterie', 'Allow iPad quality changes': 'Autoriser les réglages de qualité depuis l’iPad',
  'Every command is validated against a fixed allowlist before Windows receives it.': 'Chaque commande est validée par une liste blanche stricte avant d’être reçue par Windows.', 'Allowed input': 'Commandes autorisées', 'Keyboard & shortcuts': 'Clavier et raccourcis', 'Mouse & trackpad': 'Souris et trackpad', 'Direct touch': 'Toucher direct', 'Creative tools': 'Outils créatifs', 'Pencil mode': 'Mode crayon', 'Mouse pointer': 'Pointeur de souris', 'Detect active creative app': 'Détecter l’application créative active', 'Selects Blender, Maya or Photoshop controls automatically': 'Sélectionne automatiquement les commandes Blender, Maya ou Photoshop',
  'Zero Trust defaults protect every new device and every incoming command.': 'Les réglages Zero Trust protègent chaque nouvel appareil et chaque commande entrante.', 'Authorization': 'Autorisation', 'Confirm every connection': 'Confirmer chaque connexion', 'Trusted devices still require approval': 'Les appareils de confiance nécessitent encore une autorisation', 'Notifications': 'Notifications', 'Notify when a device connects': 'Notifier lorsqu’un appareil se connecte', 'Session lifetime': 'Durée de session', 'Trust expires after': 'Expiration de la confiance après', 'Brute-force protection': 'Protection contre les tentatives répétées', 'Invalid attempts before lock': 'Tentatives invalides avant verrouillage', 'Lockout duration': 'Durée du verrouillage', 'Unlock all': 'Tout déverrouiller', 'Security log': 'Journal de sécurité', 'Append-only local history': 'Historique local non modifiable', 'No events recorded.': 'Aucun événement enregistré.',
  'Local Only is recommended and never exposes the PC directly to the Internet.': 'Le mode local uniquement est recommandé et n’expose jamais directement le PC à Internet.', 'Direct iPad ↔ PC connection on your private network.': 'Connexion directe iPad ↔ PC sur votre réseau privé.', 'Recommended': 'Recommandé', 'Architecture reserved for strongly authenticated relay access.': 'Architecture réservée à un relais fortement authentifié.', 'Not configured': 'Non configuré', 'Discovery': 'Découverte', 'Advertise this PC nearby': 'Annoncer ce PC à proximité', 'Allow nearby pairing requests': 'Autoriser les demandes d’association proches', 'Windows approval is always required': 'L’autorisation Windows est toujours requise', 'Secure local port': 'Port local sécurisé',
  'These controls can affect availability and troubleshooting. Secure defaults are recommended.': 'Ces réglages peuvent affecter la disponibilité et le diagnostic. Les valeurs sécurisées sont recommandées.', 'Startup': 'Démarrage', 'Start service with Vision-Pro': 'Démarrer le service avec Vision-Pro', 'Launch at Windows sign-in': 'Lancer à l’ouverture de session Windows', 'Diagnostics': 'Diagnostics', 'Verbose local logging': 'Journalisation locale détaillée', 'Developer loopback endpoint': 'Point d’accès loopback développeur', '127.0.0.1 only; disabled in normal use': '127.0.0.1 uniquement ; désactivé en usage normal',
  'Your computer. Everywhere.': 'Votre ordinateur. Partout.', 'Development code name': 'Nom de code de développement', 'This PC': 'Ce PC', 'Identity fingerprint': 'Empreinte de l’identité', 'Local hostname': 'Nom d’hôte local',
  'Connection request': 'Demande de connexion', 'VERIFY CODE': 'VÉRIFIER LE CODE', 'Decline': 'Refuser', 'Allow': 'Autoriser', 'Secure pairing': 'Association sécurisée', 'Connect your iPad': 'Connectez votre iPad', 'Close': 'Fermer', 'PAIRING CODE': 'CODE D’ASSOCIATION', 'New code': 'Nouveau code', 'Expires': 'Expire', 'Discovery is not authorization.': 'La découverte n’est pas une autorisation.',
  'is controlling': 'contrôle', 'is viewing': 'visualise', 'this PC.': 'ce PC.', 'controlling': 'contrôle', 'viewing': 'visualise',
  'Apple Pencil': 'Apple Pencil', 'Keyboard and shortcuts': 'Clavier et raccourcis', 'Mouse and trackpad': 'Souris et trackpad', 'Detect active app': 'Détecter l’application active',
  'minutes': 'minutes', 'days': 'jours', 'attempts': 'tentatives', 'blocked source': 'source bloquée', 'blocked sources': 'sources bloquées',
  'Off': 'Désactivé', 'Unreachable': 'Injoignable', 'Connecting': 'Connexion', 'Relay': 'Relais', 'Relay address': 'Adresse du relais', 'Access token': 'Jeton d’accès', 'Optional': 'Facultatif', 'Save': 'Enregistrer',
  'Reachable from the Internet': 'Joignable depuis Internet', 'Relay unreachable': 'Relais injoignable', 'Connecting to the relay…': 'Connexion au relais…', 'Relay inactive': 'Relais inactif',
  'Encrypted media over UDP': 'Flux chiffré via UDP', 'Encrypted media over TCP (UDP blocked)': 'Flux chiffré via TCP (UDP bloqué)', 'Save to connect': 'Enregistrez pour vous connecter', 'Enter your relay address': 'Saisissez l’adresse de votre relais',
  'Local Only never exposes the PC to the Internet. Remote Access goes through your own relay, with end-to-end encryption.': 'Le mode local n’expose jamais le PC à Internet. L’accès distant passe par votre propre relais, avec un chiffrement de bout en bout.',
  'Reach this PC from any network through a Vision-Pro relay. The relay never sees your screen or keystrokes.': 'Accédez à ce PC depuis n’importe quel réseau via un relais Vision-Pro. Le relais ne voit jamais votre écran ni vos frappes.',
  'Pair each iPad once on the local network. It then connects from anywhere: the PC key is pinned, pairing codes are always required through the relay, and video stays DTLS-SRTP encrypted.': 'Associez chaque iPad une fois sur le réseau local. Il se connecte ensuite depuis n’importe où : la clé du PC est épinglée, un code d’association est toujours exigé via le relais et la vidéo reste chiffrée en DTLS-SRTP.',
  'Enter the relay address, for example relay.example.com.': 'Saisissez l’adresse du relais, par exemple relay.exemple.com.', 'The access token must contain at least 16 characters.': 'Le jeton d’accès doit contenir au moins 16 caractères.',
  'Vision-Pro uses a direct encrypted WebRTC path on the local network. Video does not transit through a cloud service in Local Only mode.': 'Vision-Pro utilise un chemin WebRTC chiffré et direct sur le réseau local. La vidéo ne transite par aucun service cloud en mode Local uniquement.',
};

function relative(value: string): string {
  return value
    .replace(/^Just now$/, 'à l’instant')
    .replace(/^(\d+) min ago$/, 'il y a $1 min')
    .replace(/^(\d+) h ago$/, 'il y a $1 h')
    .replace(/^(\d+) d ago$/, 'il y a $1 j');
}

function translate(value: string): string {
  const direct = french[value];
  if (direct) return direct;
  const connected = /^Last connected (.+)$/.exec(value);
  if (connected) return `Dernière connexion ${relative(connected[1]!)}`;
  const expires = /^Expires (.+)$/.exec(value);
  if (expires) return `Expire ${relative(expires[1]!)}`;
  return relative(value).replace(/^(\d+) trusted$/, '$1 de confiance').replace(/^(\d+) FPS sent$/, '$1 FPS envoyées').replace(/ · via relay$/, ' · via le relais');
}

function apply(root: Element) {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const nodes: Text[] = [];
  while (walker.nextNode()) nodes.push(walker.currentNode as Text);
  for (const node of nodes) {
    const value = node.nodeValue ?? ''; const compact = value.trim();
    if (compact) node.nodeValue = value.replace(compact, translate(compact));
  }
  for (const element of [root, ...root.querySelectorAll('*')]) {
    for (const attribute of ['aria-label', 'placeholder', 'title', 'alt']) {
      const value = element.getAttribute(attribute);
      if (value) element.setAttribute(attribute, translate(value));
    }
  }
}

export function useDesktopLanguage(): void {
  useEffect(() => {
    document.documentElement.lang = 'fr';
    localStorage.removeItem('vision-pro-desktop-language');
    const localize = () => apply(document.body);
    localize();
    const observer = new MutationObserver(localize);
    observer.observe(document.body, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, []);
}
