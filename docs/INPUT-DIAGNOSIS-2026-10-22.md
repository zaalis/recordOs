# Diagnostic Pencil / navigation Blender

Mesures locales : diagnostics/input-20261022-184453/ (exclues de Git). Le script scripts/measure-live-input.py relève à 20 Hz la position du curseur, le PID au premier plan et le temps CPU de Vision-Pro ; il mesure séparément le RTT HTTPS PC-relais. Il ne lit ni frappes clavier, ni contenu de fenêtre, ni images.

Résultats :

```json
{
  "samples": 2384,
  "duration_s": 119.96,
  "blender_foreground_s_approx": 62.24,
  "cursor_steps_over_300px": 8,
  "max_cursor_step_px": 864.92,
  "agent_cpu_seconds": 1.1406,
  "pc_relay_warm_https_rtt_median_ms": 99.43,
  "user_reported_commands_estimate_ms": 218,
  "implied_application_rtt_ms_approx": 436,
  "limits": "Cursor jumps are observed, not attributed. Sampled PC-to-relay HTTPS RTT is not iPad-to-PC command latency. No session disconnect reason captured."
}
```

Les gros déplacements peuvent inclure un mouvement volontaire de souris ou le recentrage de Blender ; ils ne prouvent pas leur origine iPad. L'indicateur Commandes vaut actuellement RTT/2 : les 218 ms déclarées ne sont pas une mesure d'injection réelle. Aucun motif de déconnexion de la session utilisateur n'est disponible dans ce relevé.

## Constats dans les sources

- penMove accepte les positions de survol même quand un geste des doigts navigue dans la scène. penDown efface les doigts sans terminer explicitement le geste 3D et ses modificateurs. Ces transitions doivent être sérialisées pour éviter deux sources pilotant le même pointeur ou un bouton maintenu.
- begin3DGesture et les mises à jour orbit/pan envoient des coordonnées absolues. Blender peut recentrer le curseur pendant un geste : une navigation relative dédiée doit être testée pour éviter de le ramener vers la position du doigt.
- Le regroupement temps réel ne conserve que le dernier paquet. Il convient aux positions absolues ; des déplacements relatifs doivent être accumulés ou encodés avec un cumul afin de ne pas perdre de distance.
- Le relais ferme le WebSocket au-delà de 400 trames clientes par seconde. Le dépassement est un mécanisme présent dans le code ; il n'a pas encore été prouvé pour les coupures signalées. Il faut mesurer les trames, la file et la raison de fermeture avant de modifier le seuil.
- Le relais est en us-east1-b ; les mesures HTTPS locales donnent environ 99 ms aller-retour PC-relais, sans mesure de la partie iPad. Une implantation européenne pourrait réduire ce trajet, mais ne corrigera pas les erreurs de gestes.

## Correction et validation proposées

1. Ajouter une télémétrie de session bornée : cadence d'entrée, file de chiffrement/envoi, âge des événements, fermeture WebSocket avec raison et durée de session. Ne pas journaliser de frappes ni de contenu.
2. Gérer une source de pointeur active : suspendre le survol Pencil pendant la navigation des doigts ; terminer bouton/modificateurs avant de reprendre le Pencil. Stabiliser aussi le contrôleur pendant le masquage des outils.
3. Tester la navigation Blender en relatif, en conservant la distance totale et en gardant les pressions/relâchements ordonnés.
4. Borner la cadence des mouvements et traiter les pointes sans déconnexion d'un flux normal, avec une protection anti-abus conservée. Tester à 60/120/240 Hz et avec plusieurs échantillons coalescés.
5. Exécuter trois séquences séparées : Pencil seul, doigts seuls en navigation, alternance des deux. Comparer avant/après sur la même scène Blender. Vérifier l'absence de touche/bouton maintenu, les déplacements pendant l'arrêt du geste, les reconnexions et les distributions de RTT.
6. Étudier le relais européen après stabilisation du flux, sans déplacer la VM ou modifier la session pendant les mesures de référence.

Cette phase prépare le diagnostic et les mesures. Aucun nouveau correctif de contrôle, installateur ou déploiement de production n'est annoncé comme validé.
