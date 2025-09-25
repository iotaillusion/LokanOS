'use strict';

const { createApp } = require('./app');

const port = Number.parseInt(process.env.PORT, 10) || 4400;
const app = createApp();

app.listen(port, () => {
  // eslint-disable-next-line no-console
  console.log(`rule-engine service listening on ${port}`);
});
