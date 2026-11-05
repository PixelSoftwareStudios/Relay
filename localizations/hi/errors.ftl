# Relay desktop UI catalog, Hindi (hi), section: Errors the UI words itself.
# Terms: call=कॉल, mute=म्यूट, deafen=सुनना बंद, peer=पीयर, channel=चैनल, invite=आमंत्रण, trust=भरोसा, file offer=फ़ाइल प्रस्ताव.
# Same ids and $variables as localizations/en/errors.ftl. See docs/i18n.md.

err-not-connected = Relay डेमन से नहीं जुड़ा है
err-release-delay = छोड़ने की देरी 0 से 2000 ms के बीच होनी चाहिए
err-gain = गेन 0 से 4 के बीच की सीमित संख्या होनी चाहिए
err-bad-call-id = कॉल आईडी गलत रूप में है
err-unknown-call = अज्ञात कॉल
err-no-lease = इंजन ने कोई नियंत्रण लीज़ नहीं दी
err-bad-peer-id = पीयर आईडी गलत रूप में है
err-unknown-peer = अज्ञात पीयर
err-consent-required = आवाज़ भेजने के लिए माइक्रोफ़ोन की सहमति ज़रूरी है
err-ring-volume = रिंगटोन की आवाज़ 0 से 1 के बीच होनी चाहिए
err-unknown-device = अज्ञात या अनुपलब्ध ऑडियो डिवाइस
err-display-name = प्रदर्शित नाम 1 से 64 प्रिंट होने योग्य अक्षरों का होना चाहिए
err-avatar-media-type = प्रोफ़ाइल चित्र के लिए मीडिया प्रकार चाहिए
conn-scheme = कनेक्शन स्ट्रिंग relay:// से शुरू होती है
err-invite-malformed = वह आमंत्रण टोकन गलत रूप में है
err-join-target = आमंत्रण टोकन या 32 अक्षरों की चैनल आईडी डालें
err-bad-channel-id = चैनल आईडी गलत रूप में है
err-unknown-channel = अज्ञात चैनल
err-channel-name = चैनल का नाम 1 से 32 प्रिंट होने योग्य अक्षरों का होना चाहिए
err-channel-needs-password = पासवर्ड वाले चैनल के लिए पासवर्ड चाहिए
err-enter-password = चैनल का पासवर्ड डालें
err-invite-admin-only = सिर्फ़ मालिक या एडमिन आमंत्रण बना सकता है
err-invite-range = आमंत्रण की अवधि या उपयोग की संख्या सीमा से बाहर है
err-no-invite = इंजन ने कोई आमंत्रण नहीं दिया
err-revoke-admin-only = सिर्फ़ मालिक या एडमिन सदस्यों को हटा सकता है
err-not-member = वह पीयर चैनल का प्रमाणित सदस्य नहीं है
err-nothing-to-change = बदलने के लिए कुछ नहीं है
err-bad-avatar-hash = प्रोफ़ाइल चित्र का हैश गलत रूप में है
conn-missing-fingerprint = कनेक्शन स्ट्रिंग में #फ़िंगरप्रिंट वाला हिस्सा नहीं है।
conn-bad-fingerprint = फ़िंगरप्रिंट 64 हेक्साडेसिमल अक्षरों का होना चाहिए।
conn-bad-address = पता सीधा ip:port होना चाहिए, नाम नहीं।
watch-resync = इंजन ने फिर से सिंक करने को कहा: { $reason }
watch-closed = डेमन ने कनेक्शन बंद कर दिया
err-unsupported = यह इंजन उस अनुरोध को समर्थन नहीं देता
err-daemon-unreachable = Relay डेमन तक पहुँचा नहीं जा सकता: { $reason }
err-engine-timeout = इंजन ने समय पर जवाब नहीं दिया
err-connection = कनेक्शन की समस्या: { $reason }
err-unreadable-image = वह फ़ाइल पढ़ने योग्य चित्र नहीं है।
err-engine-rejected = इंजन ने अनुरोध अस्वीकार कर दिया
err-no-paths = यह डिवाइस Relay डेमन के लिए फ़ाइलें या फ़ोल्डर नहीं चुन सकता।
err-cannot-start-daemon = यह डिवाइस Relay डेमन शुरू नहीं कर सकता।
err-cannot-stop-daemon = यह डिवाइस Relay डेमन बंद नहीं कर सकता।
err-read-file = फ़ाइल पढ़ी नहीं जा सकी: { $error }
