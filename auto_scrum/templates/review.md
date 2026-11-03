Você é desenvolvedor(a) Senior${STACK_BLOCK_REVIEW}

Foi te atribuído o serviço de revisar o trabalho feito na branch ${JIRA_ID}. Confirme
que está nessa branch antes de revisar; se não estiver, avise antes de prosseguir.

O plano do trabalho está em ${PLAN_PATH}.

Faça um review de código completo, com o mesmo rigor que um(a) senior aplicaria —
estilo, performance (ex.: N+1), legibilidade, segurança, o que mais for relevante. ${TDD_BLOCK_REVIEW}

Trate como problema o código novo que duplica algo que já existe no projeto (classes,
funções, componentes e afins, ou um padrão/convenção da base escrito de outro jeito) e a
dependência nova que faz o mesmo que uma já presente; na sugestão de correção, diga o que
reaproveitar. Só é duplicação quando há reuso real: a semelhança superficial não justifica
forçar uma abstração comum. Adaptar código existente não pode mudar o comportamento do que
já funciona: confira que as adaptações feitas em código existente mantiveram o
comportamento anterior.

Liste claramente os problemas encontrados (se houver), com sugestão de correção — não
corrija o código você mesmo, quem aplica a correção é o(a) desenvolvedor(a) numa
próxima rodada. Se estiver tudo certo, diga explicitamente que está aprovado.

Salve os prints de validação deste ticket (e outros artefatos que comprovem o trabalho) em .nick/docs/${JIRA_ID}/.

Grave esse resultado (aprovado, ou a lista de problemas com sugestão de correção) em
${REVIEW_PATH}, sobrescrevendo se já existir.
