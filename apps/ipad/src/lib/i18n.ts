import { useEffect } from 'react';

// Source copy is translated at the presentation boundary. Protocol fields and
// device data remain language-neutral, while the application itself is French-only.
const french: Record<string, string> = {
  'Settings': 'Réglages', 'Appearance': 'Apparence', 'Dark': 'Sombre', 'Light': 'Clair',
  'Close': 'Fermer',
  'Your computer.': 'Votre ordinateur.', 'Everywhere.': 'Partout.',
  'Sign in': 'Se connecter', 'Create account': 'Créer un compte', 'Get Started': 'Commencer',
  'Name this iPad': 'Nommer cet iPad', 'Studio iPad': 'iPad Studio',
  'End-to-end encrypted. No account needed on your own network.': 'Chiffré de bout en bout. Aucun compte nécessaire sur votre réseau.',
  'Nearby computers': 'Ordinateurs à proximité', 'Refresh': 'Actualiser', 'No computers found': 'Aucun ordinateur trouvé',
  'Open Vision-Pro on your Mac and choose Connect Device, then scan the QR code with this iPad.': 'Ouvrez Vision-Pro sur votre Mac, choisissez Connecter un appareil, puis scannez le code QR avec cet iPad.',
  'Ready': 'Prêt', 'Ready to pair': 'Prêt à associer', 'On this network': 'Sur ce réseau', 'Paired': 'Associé', 'Connect': 'Connecter', 'Open': 'Ouvrir',
  'Add computer': 'Ajouter un ordinateur', 'Enter the code shown on your Mac': 'Saisissez le code affiché sur votre Mac',
  'Scan QR Code': 'Scanner le code QR', 'Pair in one step': 'Associer en une étape',
  'Direct, end-to-end encrypted connection': 'Connexion directe, chiffrée de bout en bout',
  'This iPad': 'Cet iPad', 'The name is shown on the Mac when this iPad requests access.': 'Ce nom est affiché sur le Mac lorsque cet iPad demande un accès.',
  'Device name': 'Nom de l’appareil', 'Experience': 'Expérience', 'Automatic creative profiles': 'Profils créatifs automatiques',
  'Match controls to the active Mac app': 'Adapte les commandes à l’application active du Mac', 'Performance statistics': 'Statistiques de performances',
  'Show remote cursor': 'Afficher le curseur distant', 'Hide controls when idle': 'Masquer les commandes au repos',
  'Paired computers': 'Ordinateurs associés', 'No paired computers': 'Aucun ordinateur associé', 'Forget': 'Oublier',
  "Removing a computer only clears this iPad's local trust record. Revoke the iPad from the Mac to invalidate its key immediately.": 'Retirer un ordinateur efface uniquement l’autorisation locale de cet iPad. Révoquez l’iPad depuis le Mac pour invalider sa clé immédiatement.',
  'Computer removed from this iPad.': 'Ordinateur retiré de cet iPad.', 'Privacy & security': 'Confidentialité et sécurité',
  'Private key': 'Clé privée', 'Protected': 'Protégée', 'Reset this iPad': 'Réinitialiser cet iPad', 'Confirm reset': 'Confirmer la réinitialisation', 'Cancel': 'Annuler',
  'Non-extractable and stored only on this iPad': 'Non exportable et stockée uniquement sur cet iPad', 'Resetting creates a new device identity. Every Mac will require explicit pairing again.': 'La réinitialisation crée une nouvelle identité d’appareil. Chaque Mac demandera à nouveau une association explicite.',
  'Connection stopped': 'Connexion interrompue', 'Confirm on your Mac': 'Confirmez sur votre Mac', 'Reconnecting': 'Reconnexion', 'Starting the display': 'Démarrage de l’affichage', 'Connecting securely': 'Connexion sécurisée',
  'Back to computers': 'Retour aux ordinateurs', 'Waiting for the first secure video frame…': 'En attente de la première image vidéo sécurisée…',
  'View only — input is disabled on the Mac': 'Lecture seule — les commandes sont désactivées sur le Mac', 'Reconnecting securely…': 'Reconnexion sécurisée…',
  'Session controls': 'Commandes de session', 'Move toolbar. Drag, or use arrow keys.': 'Déplacer la barre. Faites-la glisser ou utilisez les flèches.',
  'Home': 'Accueil', 'Displays': 'Écrans', 'Control': 'Contrôle', 'Keyboard': 'Clavier', 'Shortcuts': 'Raccourcis', 'Quality': 'Qualité', 'Hide controls': 'Masquer les commandes', 'Show controls': 'Afficher les commandes',
  'Enter fullscreen': 'Activer le plein écran', 'Exit fullscreen': 'Quitter le plein écran', 'Fingers': 'Doigts', 'Trackpad or mouse': 'Trackpad ou souris',
  'Brushes': 'Brosses', 'Tools': 'Outils', 'View': 'Vue', 'Select': 'Sélectionner', 'Translate': 'Déplacer', 'Frame selected': 'Cadrer la sélection', 'Frame all': 'Tout cadrer', 'Invert mask': 'Inverser le masque',
  'Streaming': 'Diffusion', 'Session': 'Session', 'Active': 'Actif', 'Use': 'Utiliser', 'Direct Touch': 'Toucher direct', 'Trackpad': 'Trackpad', 'Pencil': 'Crayon',
  'Open iPad keyboard': 'Ouvrir le clavier de l’iPad', 'Resolution': 'Définition', 'Frame rate': 'Fréquence d’images', 'Encoder': 'Encodeur', 'Network': 'Réseau', 'Hardware': 'Matériel', 'Software': 'Logiciel',
  'Statistics overlay': 'Panneau de statistiques', 'Remote cursor': 'Curseur distant', 'Fullscreen': 'Plein écran', 'Connection': 'Connexion', 'Encryption': 'Chiffrement', 'Path': 'Chemin', 'Disconnect': 'Déconnecter',
  'Connection statistics': 'Statistiques de connexion', 'Live': 'En direct', 'Captured': 'Capturé', 'Sent': 'Envoyé', 'Loss': 'Perte',
  'Another computer': 'Un autre ordinateur', 'Pair securely': 'Associer de manière sécurisée',
  'Accounts are optional. Vision-Pro works completely on your local network: your iPad and Mac connect directly and nothing passes through the cloud.': 'Les comptes sont facultatifs. Vision-Pro fonctionne entièrement sur votre réseau local : l’iPad et le Mac se connectent directement, sans passage par le cloud.',
  'All your Macs in one place': 'Tous vos Mac au même endroit', 'Synced preferences': 'Préférences synchronisées', 'Name': 'Nom', 'Email': 'E-mail', 'Password': 'Mot de passe',
  'New to Vision-Pro? Create an account': 'Nouveau sur Vision-Pro ? Créer un compte', 'Already have an account? Sign in': 'Vous avez déjà un compte ? Se connecter',
  'On your Mac, choose Connect Device and point this iPad at the code.': 'Sur votre Mac, choisissez Connecter un appareil, puis cadrez le code avec cet iPad.',
  'Camera access is needed to scan. You can also scan the code with the Camera app.': 'L’accès à la caméra est nécessaire pour scanner. Vous pouvez aussi utiliser l’appareil photo de l’iPad.',
  'Digit': 'Chiffre', 'Attempt': 'Tentative', 'of': 'sur',
  'Verify that this code matches the one shown by Vision-Pro on the Mac, then approve the request there.': 'Vérifiez que ce code correspond à celui affiché par Vision-Pro sur le Mac, puis autorisez la demande sur le Mac.',
  'Discovering the fastest private path to your computer.': 'Recherche du chemin privé le plus rapide vers votre ordinateur.',
  'The Mac could not start streaming.': 'Le Mac n’a pas pu démarrer la diffusion.',
  'The session ended on the Mac.': 'La session a pris fin sur le Mac.', 'Type on the Mac': 'Saisir sur le Mac',
  'Pressure & tilt': 'Pression et inclinaison', 'macOS tablet pen': 'Stylet de tablette macOS',
  'This page is served directly by your Mac. To use an account, open Vision-Pro from your account server.': 'Cette page est servie directement par votre Mac. Pour utiliser un compte, ouvrez Vision-Pro depuis votre serveur de comptes.',
  'Open this app from the address shown on your Mac to add it.': 'Ouvrez cette app depuis l’adresse affichée sur votre Mac pour l’ajouter.',
  // Built-in macOS shortcut profiles.
  'Edit': 'Édition', 'Undo': 'Annuler', 'Redo': 'Rétablir', 'Cut': 'Couper', 'Copy': 'Copier', 'Paste': 'Coller', 'Select All': 'Tout sélectionner',
  'Save': 'Enregistrer', 'Find': 'Rechercher', 'Switch App': 'Changer d’app', 'App Windows': 'Fenêtres de l’app', 'Show Desktop': 'Afficher le bureau',
  'Previous Space': 'Bureau précédent', 'Next Space': 'Bureau suivant', 'Screenshot': 'Capture d’écran', 'Close Window': 'Fermer la fenêtre',
  'Minimize': 'Placer dans le Dock', 'Hide App': 'Masquer l’app', 'Quit App': 'Quitter l’app', 'Force Quit': 'Forcer à quitter', 'Lock Screen': 'Verrouiller l’écran',
  'Media': 'Médias', 'Play / Pause': 'Lecture / Pause', 'Previous': 'Précédent', 'Next': 'Suivant', 'Mute': 'Muet',
};

function translateValue(value: string): string {
  const direct = french[value];
  if (direct) return direct;
  return value
    .replace(/^Last used /, 'Utilisé pour la dernière fois ')
    .replace(/ min ago$/, ' min auparavant')
    .replace(/ hr ago$/, ' h auparavant')
    .replace(/ days ago$/, ' jours auparavant');
}

function localizeElement(root: Element): void {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const nodes: Text[] = [];
  while (walker.nextNode()) nodes.push(walker.currentNode as Text);
  for (const node of nodes) {
    const source = node.nodeValue ?? '';
    const trimmed = source.trim();
    if (!trimmed) continue;
    const translated = translateValue(trimmed);
    if (translated !== trimmed) node.nodeValue = source.replace(trimmed, translated);
  }
  for (const element of [root, ...root.querySelectorAll('*')]) {
    for (const attribute of ['aria-label', 'placeholder', 'title']) {
      const value = element.getAttribute(attribute);
      if (value) element.setAttribute(attribute, translateValue(value));
    }
  }
}

export function useLocalizedInterface(): void {
  useEffect(() => {
    document.documentElement.lang = 'fr';
    const apply = () => localizeElement(document.body);
    apply();
    const observer = new MutationObserver(apply);
    observer.observe(document.body, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, []);
}
