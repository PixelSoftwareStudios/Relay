# Relay desktop UI catalog, Hindi (hi), section: Calls: phases, transmit status, trust.
# Terms: call=कॉल, mute=म्यूट, deafen=सुनना बंद, peer=पीयर, channel=चैनल, invite=आमंत्रण, trust=भरोसा, file offer=फ़ाइल प्रस्ताव.
# Same ids and $variables as localizations/en/calls.ftl. See docs/i18n.md.

phase-preparing = तैयारी हो रही है
phase-ringing = घंटी बज रही है
phase-incoming = आने वाली कॉल
phase-connecting = जुड़ रहा है
phase-connected = जुड़ा हुआ
phase-interrupted = बाधित
phase-ending = समाप्त हो रही है
phase-ended = समाप्त
phase-failed = विफल
phase-declined = अस्वीकृत
phase-cancelled = रद्द
transmit-listening = सिर्फ़ सुन रहे हैं
transmit-listening-detail = आपका माइक्रोफ़ोन भेजा नहीं जा रहा।
transmit-no-consent = माइक्रोफ़ोन की अनुमति नहीं है
transmit-no-consent-detail = इस कॉल के लिए आवाज़ भेजने की सहमति नहीं दी गई है।
transmit-releasing = छोड़ा जा रहा है
transmit-releasing-detail = कुंजी छोड़ दी गई है; वाक्य का अंत कट न जाए, इसलिए माइक्रोफ़ोन थोड़ी देर चालू रहता है।
transmit-how-open = खुला माइक्रोफ़ोन
transmit-how-key = पुश-टू-टॉक कुंजी दबी हुई है
transmitting = आवाज़ भेजी जा रही है
transmit-live-detail = माइक्रोफ़ोन चालू है ({ $how })।
transmit-open-armed = खुला माइक तैयार है, आवाज़ नहीं भेजी जा रही
transmit-open-armed-detail = इंजन अभी ऑडियो नहीं भेज रहा है।
transmit-ptt-armed = पुश-टू-टॉक तैयार है
transmit-ptt-armed-detail = इंजन की ओर के इनपुट स्रोत से कुंजी दबाए जाने का इंतज़ार है।
engine-reason = इंजन का कारण: { $reason }।
trust-unknown = अज्ञात
trust-tofu = पहले देखा गया, सत्यापित नहीं
trust-verified = सत्यापित
trust-key-changed = कुंजी बदल गई: भरोसा न करें
trust-blocked = ब्लॉक किया गया
call-speaking = बोल रहे हैं
mode-listen = सिर्फ़ सुनें
mode-ptt = पुश-टू-टॉक
mode-open = खुला माइक
calls-heading = कॉल
meter-incoming = { $name } से आने वाला स्तर
call-accept = स्वीकार करें
call-decline = अस्वीकार करें
call-accept-note = स्वीकार करने से आपका माइक्रोफ़ोन चालू नहीं होता।
trust-line = { $trust }।
trust-line-key = { $trust }। डिवाइस कुंजी { $fingerprint } (डिवाइस पहचानने के लिए, सत्यापन के लिए नहीं)।
call-end = कॉल समाप्त करें
call-messages-hover = इस कॉल के लिए टेक्स्ट, चित्र और फ़ाइलें
call-messages = संदेश
call-messages-unread = संदेश ({ $count })
history-messages-count = संदेश: { $count }
history-messages-gone = इस कॉल के संदेश अब सहेजे नहीं जाते
call-unmute = अनम्यूट करें (कॉल)
call-undeafen = सुनना चालू करें (कॉल)
call-their-volume = उनकी आवाज़ { $value }
call-your-mic = इस कॉल में आपका माइक्रोफ़ोन
call-consent = { $name } को मेरा माइक्रोफ़ोन भेजे जाने की अनुमति है
call-mute = म्यूट करें (कॉल)
call-deafen = सुनना बंद करें (कॉल)
call-talk-hold = बोलने के लिए दबाए रखें
incoming-title = { $name } कॉल कर रहे हैं
call-mic-help = ऊपर का बॉक्स चुनने तक आपका माइक्रोफ़ोन बंद रहेगा।
calls-sounds = ध्वनियाँ
ring-help = ध्वनियाँ केवल इस कंप्यूटर पर बजती हैं।
history-heading = इतिहास
history-empty = अभी तक कोई कॉल इतिहास नहीं
history-empty-help = आपके किए और प्राप्त किए गए कॉल यहाँ सूचीबद्ध होंगे।
history-off-note = कॉल इतिहास बंद है। कुछ नया दर्ज नहीं होता; नीचे की प्रविष्टियाँ आपके हटाने तक बनी रहती हैं।
history-incoming = आने वाला
history-outgoing = जाने वाला
history-missed = छूटा हुआ
history-declined = अस्वीकृत
history-not-reached = संपर्क नहीं हुआ
history-cancelled = रद्द किया गया
history-call-back = वापस कॉल करें
history-show-more = और दिखाएँ
history-clear = इतिहास हटाएँ
history-clear-ask = इस डिवाइस का पूरा कॉल इतिहास हटाएँ?
history-clear-yes = इतिहास हटाएँ
history-clear-keep = रहने दें
history-time-now = अभी
history-time-minutes = { $count } मिनट पहले
history-time-hours = { $count } घंटे पहले
history-time-yesterday = कल, { $time }
history-time-days =
    { $count ->
        [one] 1 दिन पहले
       *[other] { $count } दिन पहले
    }

mic-ask-title = Relay को आपके माइक्रोफ़ोन की ज़रूरत है
mic-ask-body = माइक्रोफ़ोन का उपयोग करने दें, ताकि कॉल में दूसरा व्यक्ति आपको सुन सके।
mic-allow = माइक्रोफ़ोन की अनुमति दें
mic-off-title = माइक्रोफ़ोन का उपयोग बंद है
mic-off-body = इसके बिना कॉल में कोई आपको नहीं सुन पाएगा। इसे सिस्टम सेटिंग में चालू करें और फिर वापस आएँ।
mic-open-settings = सिस्टम सेटिंग खोलें
