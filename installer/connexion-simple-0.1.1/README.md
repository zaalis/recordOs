# Vision-Pro 0.1.1 — connexion simple

1. Quittez l'ancienne application Vision-Pro (y compris son icône près de l'horloge).
2. Lancez `Vision-Pro_0.1.1_x64-setup.exe`, puis ouvrez Vision-Pro.
3. Sur le PC, choisissez « Connecter un iPad ».
4. Sur l'iPad, ouvrez https://vision-pro.34-73-186-88.sslip.io dans Safari. Rechargez la page si elle était déjà ouverte.
5. Choisissez « Connecter un ordinateur » et saisissez les 4 chiffres affichés sur le PC.
6. Vérifiez le code de comparaison affiché sur les deux appareils, puis autorisez sur le PC.

L'iPad est ensuite mémorisé : choisissez simplement votre ordinateur pour vous reconnecter. Aucun compte, profil ou certificat à installer sur l'iPad. Les appareils peuvent utiliser deux réseaux différents. Le PC doit rester allumé, Vision-Pro lancé et Internet disponible sur les deux appareils.

Les codes expirent après deux minutes ; « Nouveau code » permet de recommencer. Un code ne sert qu'une fois. Après plusieurs erreurs, générez un nouveau code ; si les essais sont trop rapprochés, patientez une minute.

Le fichier `vision-pro.exe` est la version portable : c'est une autre manière de lancer la même application, ne lancez pas les deux versions simultanément.

L'installateur a été compilé, mais n'a pas été exécuté automatiquement. Les binaires ne sont pas signés Authenticode ; Windows peut afficher un avertissement d'éditeur inconnu. Les empreintes des fichiers se trouvent dans SHA256SUMS.txt.

Tests effectués sur le site HTTPS public et dans Edge au format iPad. Le test sur un iPad physique et Safari reste à faire par l'utilisateur.
