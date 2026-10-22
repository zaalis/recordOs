# Record OS 0.1.4 — installateur et identité du produit

Le produit s’affiche désormais sous le nom **Record OS** dans l’application Windows, l’interface iPad, les titres de fenêtres, le manifeste PWA et les textes de connexion. Les fichiers du logo d’origine sont inchangés.

## Installation Windows

Le livrable principal est `installer/Record-OS-0.1.4/Record-OS_0.1.4_x64-setup.exe`. Il contient une interface WPF française, avec un manifeste DPI PerMonitorV2, une typographie système et des contrôles adaptés à la mise à l’échelle. L’installation s’appuie sur le moteur NSIS généré par Tauri, embarqué dans le même fichier. Aucun moteur d’installation maison ne remplace la copie et l’enregistrement Windows standard.

Le lanceur vérifie le SHA-256 du moteur embarqué, détecte l’installation précédente dans le registre, conserve son emplacement et refuse de poursuivre pendant qu’une instance de l’application est ouverte dans la session Windows. Le moteur NSIS vérifie aussi ce dernier point avant de copier les fichiers. L’étape de réussite dépend du code de sortie, de la présence du binaire, de sa version et de son enregistrement Windows. La barre de progression est indéterminée ; elle ne présente pas de pourcentage artificiel.

Les anciens raccourcis et l’ancienne entrée de désinstallation sont retirés uniquement après copie, et uniquement si l’ancien emplacement enregistré correspond à l’emplacement mis à jour. L’identité cryptographique, les appareils associés, IndexedDB et le dossier de données historique sont conservés. Le nom technique de l’exécutable, l’identifiant Tauri et les étiquettes cryptographiques restent compatibles. Ces noms internes ne sont pas une nouvelle marque utilisateur.

## Vérifications réalisées

- Compilation de l’interface iPad et de l’application Windows/Tauri ; compilation du lanceur WPF et de son moteur NSIS.
- TypeScript : vérification des types de tous les espaces de travail.
- Tests JavaScript/TypeScript : 28 réussis.
- Tests de l’agent Rust : 47 réussis, dont le stockage des autorisations et DPAPI.
- Vérification du SHA-256 embarqué par le livrable final : réussie.
- Lancement du livrable final en mode capture, sans installation ; rendu réel des écrans Bienvenue et Personnalisation à 150 %, inspecté visuellement.
- Métadonnées du binaire : ProductName et FileDescription `Record OS`, ProductVersion `0.1.4`.

La mise à niveau complète du PC utilisateur n’a pas été exécutée : le paquet est livré pour son essai. Les écrans Installation et Réussite existent, mais les captures livrées ne prétendent pas prouver une installation terminée. Le programme n’a pas de signature Authenticode commerciale.

## Interface publique iPad

Publication du web uniquement sur l’instance vérifiée `zaalis-free-e2-micro`, projet `zaalis-504018`, identifiant immuable `8497323233124612375`. L’adresse HTTPS existante est conservée pour préserver l’origine et les appareils mémorisés.

- Release : `/opt/vision-pro/releases/20261022T174356Z-record014`.
- Retour arrière : `/opt/vision-pro/releases/20261022T170517Z-input013`.
- Archive web SHA-256 : `BDE6834965FEB32F3D55EADCE04535A662EA8D428D9B969D103B56AC08CE4452`.
- Le titre public répond `Record OS`, et le bundle `index-LiAwsT7J.js` répond HTTP 200.
- Services account, relay et nginx actifs ; aucun redémarrage requis pour ce déploiement web.

Rechargez la page sur l’iPad pour recevoir la nouvelle interface. Aucun effacement des données Safari n’est nécessaire.
