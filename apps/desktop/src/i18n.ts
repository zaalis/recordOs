import { useEffect } from 'react';

const french: Record<string, string> = {
  'Overview': 'Vue d’ensemble', 'Devices': 'Appareils', 'Display': 'Écran', 'Streaming': 'Diffusion', 'Input': 'Commandes', 'Security': 'Sécurité', 'Network': 'Réseau', 'Advanced': 'Avancé', 'About': 'À propos',
  'Ready': 'Prêt', 'Stopped': 'Arrêté', 'Local network only': 'Réseau local uniquement', 'Remote access': 'Accès distant', 'Stop service': 'Arrêter le service', 'Start service': 'Démarrer le service', 'Connect Device': 'Connecter un appareil',
  'Starting Vision-Pro…': 'Démarrage de Vision-Pro…', 'Settings saved.': 'Réglages enregistrés.',
  'The device store failed its integrity check. Every device now requires confirmation.': 'Le contrôle d’intégrité du registre des appareils a échoué. Chaque appareil nécessite désormais une confirmation.',
  'Status': 'État', 'Connected': 'Connecté', 'Service stopped': 'Service arrêté', 'Disconnect': 'Déconnecter', 'Connected device': 'Appareil connecté', 'None': 'Aucun', 'Latency': 'Latence', 'Protected': 'Protégé', 'Local Only': 'Local uniquement', 'Remote Access': 'Accès distant',
  'Vision-Pro is waiting securely on your private network.': 'Vision-Pro attend de manière sécurisée sur votre réseau privé.', 'Start the service to make this Mac available.': 'Démarrez le service pour rendre ce Mac disponible.', 'No active stream': 'Aucun flux actif',
  'Live streaming': 'Diffusion en direct', 'macOS desktop': 'Bureau macOS', 'Hardware': 'Matériel', 'Software': 'Logiciel', 'Input processing': 'Traitement des commandes',
  'Recent security activity': 'Activité de sécurité récente', 'Security-relevant events from this Mac': 'Événements de sécurité de ce Mac', 'No security events yet.': 'Aucun événement de sécurité.',
  'No paired devices': 'Aucun appareil associé', 'Choose Connect Device to pair an iPad. Discovery never grants access automatically.': 'Choisissez Connecter un appareil pour associer un iPad. La découverte ne donne jamais accès automatiquement.',
  'Last connected': 'Dernière connexion', 'Never connected': 'Jamais connecté', 'Online': 'En ligne', 'Permission': 'Autorisation', 'View & Control': 'Voir et contrôler', 'View Only': 'Lecture seule', 'Last address': 'Dernière adresse', 'Key': 'Clé', 'Unavailable': 'Indisponible', 'Trust Device': 'Faire confiance', 'Rename': 'Renommer', 'Confirm device removal': 'Confirmer la suppression', 'Remove device': 'Supprimer l’appareil', 'Device name': 'Nom de l’appareil',
  'Choose what Vision-Pro streams by default.': 'Choisissez ce que Vision-Pro diffuse par défaut.', 'Default display': 'Écran par défaut', 'Primary': 'Principal', 'Use': 'Utiliser', 'Power': 'Alimentation', 'Prevent sleep while connected': 'Empêcher la veille pendant la connexion', 'Keeps the Mac and its display awake during an active session': 'Garde le Mac et son écran actifs pendant une session',
  'Automatic mode negotiates the best format supported by the Mac and iPad.': 'Le mode automatique négocie le meilleur format pris en charge par le Mac et l’iPad.', 'Vision-Pro uses a direct local WebRTC path and the Mac’s hardware media engine (VideoToolbox), applying the best format supported by both the Mac and the iPad.': 'Vision-Pro utilise un chemin WebRTC local direct et le moteur multimédia matériel du Mac (VideoToolbox), avec le meilleur format pris en charge à la fois par le Mac et l’iPad.', 'Performance': 'Performances', 'Resolution': 'Définition', 'Encoder': 'Encodeur', 'Hardware only': 'Matériel uniquement', 'Profile': 'Profil', 'Balanced': 'Équilibré', 'Low Latency': 'Faible latence', 'Maximum Quality': 'Qualité maximale', 'Battery Saver': 'Économie de batterie', 'Allow iPad quality changes': 'Autoriser les réglages de qualité depuis l’iPad',
  'Every command is validated against a fixed allowlist before macOS receives it.': 'Chaque commande est validée par une liste blanche stricte avant d’être reçue par macOS.', 'Allowed input': 'Commandes autorisées', 'Keyboard & shortcuts': 'Clavier et raccourcis', 'Mouse & trackpad': 'Souris et trackpad', 'Direct touch': 'Toucher direct', 'Creative tools': 'Outils créatifs', 'Pencil mode': 'Mode crayon', 'Tablet pen': 'Stylet (pression)', 'Mouse pointer': 'Pointeur de souris', 'Detect active creative app': 'Détecter l’application créative active', 'Selects Blender, Maya or Photoshop controls automatically': 'Sélectionne automatiquement les commandes Blender, Maya ou Photoshop',
  'Zero Trust defaults protect every new device and every incoming command.': 'Les réglages Zero Trust protègent chaque nouvel appareil et chaque commande entrante.', 'Authorization': 'Autorisation', 'Confirm every connection': 'Confirmer chaque connexion', 'Trusted devices still require approval': 'Les appareils de confiance nécessitent encore une autorisation', 'Notifications': 'Notifications', 'Notify when a device connects': 'Notifier lorsqu’un appareil se connecte', 'Session lifetime': 'Durée de session', 'Trust expires after': 'Expiration de la confiance après', 'Brute-force protection': 'Protection contre les tentatives répétées', 'Invalid attempts before lock': 'Tentatives invalides avant verrouillage', 'Lockout duration': 'Durée du verrouillage', 'Unlock all': 'Tout déverrouiller', 'Security log': 'Journal de sécurité', 'Append-only local history': 'Historique local non modifiable', 'No events recorded.': 'Aucun événement enregistré.',
  'Local Only is recommended and never exposes the Mac directly to the Internet.': 'Le mode local uniquement est recommandé et n’expose jamais directement le Mac à Internet.', 'Direct iPad ↔ Mac connection on your private network.': 'Connexion directe iPad ↔ Mac sur votre réseau privé.', 'Recommended': 'Recommandé', 'Architecture reserved for strongly authenticated relay access.': 'Architecture réservée à un relais fortement authentifié.', 'Not configured': 'Non configuré', 'Discovery': 'Découverte', 'Advertise this Mac nearby': 'Annoncer ce Mac à proximité', 'Allow nearby pairing requests': 'Autoriser les demandes d’association proches', 'Approval on this Mac is always required': 'Une autorisation sur ce Mac est toujours requise', 'Secure local port': 'Port local sécurisé',
  'These controls can affect availability and troubleshooting. Secure defaults are recommended.': 'Ces réglages peuvent affecter la disponibilité et le diagnostic. Les valeurs sécurisées sont recommandées.', 'Startup': 'Démarrage', 'Start service with Vision-Pro': 'Démarrer le service avec Vision-Pro', 'Open at login': 'Ouvrir à l’ouverture de session', 'Adds Vision-Pro to Login Items': 'Ajoute Vision-Pro aux éléments d’ouverture', 'Diagnostics': 'Diagnostics', 'Verbose local logging': 'Journalisation locale détaillée', 'Developer loopback endpoint': 'Point d’accès loopback développeur', '127.0.0.1 only; disabled in normal use': '127.0.0.1 uniquement ; désactivé en usage normal',
  'Your computer. Everywhere.': 'Votre ordinateur. Partout.', 'Development code name': 'Nom de code de développement', 'This Mac': 'Ce Mac', 'Identity fingerprint': 'Empreinte de l’identité', 'Local hostname': 'Nom d’hôte local',
  'Connection request': 'Demande de connexion', 'VERIFY CODE': 'VÉRIFIER LE CODE', 'Decline': 'Refuser', 'Allow': 'Autoriser', 'Secure pairing': 'Association sécurisée', 'Connect your iPad': 'Connectez votre iPad', 'Close': 'Fermer', 'PAIRING CODE': 'CODE D’ASSOCIATION', 'New code': 'Nouveau code', 'Expires': 'Expire', 'Discovery is not authorization.': 'La découverte n’est pas une autorisation.',
  'is controlling': 'contrôle', 'is viewing': 'visualise', 'this Mac.': 'ce Mac.', 'controlling': 'contrôle', 'viewing': 'visualise',
  'Apple Pencil': 'Apple Pencil', 'Keyboard and shortcuts': 'Clavier et raccourcis', 'Mouse and trackpad': 'Souris et trackpad', 'Detect active app': 'Détecter l’application active',
  'minutes': 'minutes', 'days': 'jours', 'attempts': 'tentatives', 'blocked source': 'source bloquée', 'blocked sources': 'sources bloquées',
  'Vision-Pro uses a direct encrypted WebRTC path on the local network. Video does not transit through a cloud service in Local Only mode.': 'Vision-Pro utilise un chemin WebRTC chiffré et direct sur le réseau local. La vidéo ne transite par aucun service cloud en mode Local uniquement.',
  'Open Vision-Pro on your iPad.': 'Ouvrez Vision-Pro sur votre iPad.', 'Scan this QR code or enter the code below.': 'Scannez ce code QR ou saisissez le code ci-dessous.', 'Confirm the matching security code on this Mac.': 'Confirmez le code de sécurité correspondant sur ce Mac.',
  'This Mac grants no control until you explicitly approve the request.': 'Ce Mac n’accorde aucun contrôle tant que vous n’approuvez pas explicitement la demande.',
  'macOS permissions': 'Autorisations macOS', 'macOS asks you to allow screen recording and input control once. Vision-Pro cannot grant them to itself.': 'macOS vous demande une seule fois d’autoriser l’enregistrement de l’écran et le contrôle. Vision-Pro ne peut pas se les accorder lui-même.',
  'Privacy & Security': 'Confidentialité et sécurité', 'After enabling a permission in System Settings, quit and reopen Vision-Pro if macOS asks you to.': 'Après avoir activé une autorisation dans Réglages Système, quittez puis rouvrez Vision-Pro si macOS le demande.',
  'Screen Recording': 'Enregistrement de l’écran', 'Required to stream this Mac’s display to the iPad': 'Nécessaire pour diffuser l’écran de ce Mac vers l’iPad',
  'Accessibility': 'Accessibilité', 'Required for the iPad to control the pointer, keyboard and Apple Pencil': 'Nécessaire pour que l’iPad contrôle le pointeur, le clavier et l’Apple Pencil',
  'Local Network': 'Réseau local', 'macOS asks once when an iPad first connects': 'macOS le demande une fois lors de la première connexion d’un iPad', 'Open Settings': 'Ouvrir les réglages',
  'Allowed': 'Autorisé', 'Allow…': 'Autoriser…', 'Review': 'Vérifier', 'macOS permission needed:': 'Autorisation macOS requise :',
};

function translate(value: string): string {
  const direct = french[value];
  if (direct) return direct;
  return value
    .replace(/^(\d+) trusted$/, '$1 de confiance')
    .replace(/^(\d+) FPS sent$/, '$1 FPS envoyées')
    .replace(/^macOS permission needed: /, 'Autorisation macOS requise : ');
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
